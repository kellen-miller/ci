//! Append GitHub environment-file records with collision-free delimiters.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

/// Append a validated output name and arbitrary value to an environment file.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn write(path: &Path, name: &str, value: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)),
        "invalid GitHub output name"
    );
    let mut output = OpenOptions::new().create(true).append(true).open(path)?;
    if value.contains(['\n', '\r']) {
        let mut delimiter = format!("ci_{}", hex::encode(Sha256::digest(value.as_bytes())));
        while value.lines().any(|line| line == delimiter) {
            delimiter.push('_');
        }

        writeln!(output, "{name}<<{delimiter}\n{value}\n{delimiter}")?;
    } else {
        writeln!(output, "{name}={value}")?;
    }

    Ok(())
}

/// Append an action output to the runner-selected file.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn append(name: &str, value: &str) -> Result<()> {
    write(Path::new(&std::env::var("GITHUB_OUTPUT")?), name, value)
}
