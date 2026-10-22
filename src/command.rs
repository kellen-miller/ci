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

/// Execute a child with inherited output, preserving failure status.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("execute {command:?}"))?;
    if !status.success() {
        return Err(Failure(status.code().unwrap_or(1)).into());
    }

    Ok(())
}

/// Capture raw stdout, retaining NUL-delimited filenames without decoding.
pub fn capture_bytes(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .output()
        .with_context(|| format!("execute {command:?}"))?;
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(Failure(output.status.code().unwrap_or(1)).into());
    }

    Ok(output.stdout)
}

/// Capture UTF-8 child output; malformed data fails rather than being rewritten.
pub fn capture(command: &mut Command) -> Result<String> {
    String::from_utf8(capture_bytes(command)?).context("child output is not UTF-8")
}
