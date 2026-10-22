//! Produce portable release archives and their verification records.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use anyhow::{Result, ensure};
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};

/// Package one platform executable with its exact archive checksum.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn package(target: &str, binary: &Path, output: &Path) -> Result<()> {
    ensure!(
        !target.is_empty()
            && target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)),
        "invalid target triple"
    );
    ensure!(
        include_str!("../runtime-version").trim() == env!("CARGO_PKG_VERSION"),
        "runtime-version must match Cargo package version"
    );
    if let Ok(reference) = std::env::var("GITHUB_REF")
        && let Some(version) = reference.strip_prefix("refs/tags/ci-v")
    {
        ensure!(
            version == env!("CARGO_PKG_VERSION"),
            "release tag must match Cargo package version"
        );
    }
    ensure!(
        binary.is_file(),
        "binary does not exist: {}",
        binary.display()
    );
    fs::create_dir_all(output)?;
    let name = format!("ci-{target}.tar.gz");
    let archive_path = output.join(&name);
    let gzip = GzEncoder::new(File::create(&archive_path)?, Compression::default());
    let mut archive = tar::Builder::new(gzip);
    let data = fs::read(binary)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(
        &mut header,
        if target.contains("windows") {
            "ci.exe"
        } else {
            "ci"
        },
        data.as_slice(),
    )?;
    archive.into_inner()?.finish()?;
    let mut hash = Sha256::new();
    let mut file = File::open(&archive_path)?;
    let mut buffer = [0_u8; 65536];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }

        hash.update(&buffer[..length]);
    }

    fs::write(
        output.join(format!("{name}.sha256")),
        format!("{}  {name}\n", hex::encode(hash.finalize())),
    )?;
    println!("Packaged {}", archive_path.display());
    Ok(())
}
