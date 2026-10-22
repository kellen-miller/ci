//! Tie a downloaded executable to the Rust sources in its selected action snapshot.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub(crate) fn digest(root: &Path) -> io::Result<String> {
    let mut files = vec![
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("build.rs"),
    ];
    let mut directories = vec![root.join("src")];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                directories.push(entry.path());
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rs")
            {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_owned();
                files.push(relative);
            }
        }
    }

    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        let name = relative.to_string_lossy().replace('\\', "/");
        let source = fs::read_to_string(root.join(&relative))?.replace("\r\n", "\n");
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((source.len() as u64).to_be_bytes());
        digest.update(source.as_bytes());
    }

    Ok(hex::encode(digest.finalize()))
}
