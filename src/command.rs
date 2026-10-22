//! Child processes retain their exit status and never evaluate shell expressions.

use std::fmt;
use std::process::Command;

use anyhow::{Context, Result};

#[derive(Debug)]
pub struct Failure(pub i32);

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "child process exited with status {}", self.0)
    }
}

impl std::error::Error for Failure {}

/// Construct a command for an ecosystem tool that may be a script shim.
/// Windows CreateProcess cannot execute the extensionless npm/Gradle scripts
/// used by Bash actions. The constant adapter keeps every argument literal.
pub fn script(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let program = std::ffi::OsString::from_wide(
            &program
                .as_ref()
                .encode_wide()
                .map(|unit| {
                    if unit == u16::from(b'\\') {
                        u16::from(b'/')
                    } else {
                        unit
                    }
                })
                .collect::<Vec<_>>(),
        );
        let mut command = Command::new("bash");
        command
            .args(["-c", "exec \"$@\"", "ci-script"])
            .arg(program);
        command
    }

    #[cfg(not(windows))]
    Command::new(program)
}

/// Execute a child with inherited output, preserving failure status.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("execute {}", command.get_program().to_string_lossy()))?;
    if !status.success() {
        return Err(Failure(status.code().unwrap_or(1)).into());
    }

    Ok(())
}

/// Capture raw stdout, retaining NUL-delimited filenames without decoding.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn capture_bytes(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .output()
        .with_context(|| format!("execute {}", command.get_program().to_string_lossy()))?;
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(Failure(output.status.code().unwrap_or(1)).into());
    }

    Ok(output.stdout)
}

/// Capture UTF-8 child output; malformed data fails rather than being rewritten.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn capture(command: &mut Command) -> Result<String> {
    String::from_utf8(capture_bytes(command)?).context("child output is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::{capture_bytes, run};
    use std::process::Command;

    #[test]
    fn spawn_failures_do_not_disclose_child_environment() {
        for capture in [false, true] {
            let mut command = Command::new("/missing/ci-fixture-executable");
            command.env("BUILD_SECRET", "secret-fixture-value");
            let result = if capture {
                capture_bytes(&mut command).map(|_| ())
            } else {
                run(&mut command)
            };

            let message = format!("{:#}", result.unwrap_err());
            assert!(message.contains("ci-fixture-executable"));
            assert!(!message.contains("secret-fixture-value"));
            assert!(!message.contains("BUILD_SECRET"));
        }
    }
}
