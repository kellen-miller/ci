//! Produce portable release archives and their verification records.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use anyhow::{Result, ensure};
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};

use crate::{command, repository};

/// Qualify a native release archive through the consumer's checksum loader.
///
/// # Errors
/// Returns a platform mismatch, invalid archive, or runtime failure.
pub fn verify_runtime(target: &str, assets: &Path) -> Result<()> {
    let (expected, runner_os, runner_arch) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => ("x86_64-unknown-linux-musl", "Linux", "X64"),
        ("linux", "aarch64") => ("aarch64-unknown-linux-musl", "Linux", "ARM64"),
        ("macos", "x86_64") => ("x86_64-apple-darwin", "macOS", "X64"),
        ("macos", "aarch64") => ("aarch64-apple-darwin", "macOS", "ARM64"),
        ("windows", "x86_64") => ("x86_64-pc-windows-msvc", "Windows", "X64"),
        platform => anyhow::bail!("unsupported native runtime platform: {platform:?}"),
    };

    ensure!(
        target == expected,
        "target {target} does not match native platform {expected}"
    );
    let temporary = tempfile::tempdir()?;
    let directory = temporary
        .path()
        .join("personal-ci")
        .join(include_str!("../runtime-version").trim())
        .join(target);
    fs::create_dir_all(&directory)?;
    let archive = format!("ci-{target}.tar.gz");
    for name in [&archive, &format!("{archive}.sha256")] {
        fs::copy(assets.join(name), directory.join(name))?;
    }

    let root = repository::root();
    command::run(
        command::bash()
            .args([".github/actions/run.sh", "workflow-security", "--root", "."])
            .current_dir(&root)
            .env_remove("CI_BINARY")
            .env("RUNNER_TEMP", temporary.path())
            .env("RUNNER_OS", runner_os)
            .env("RUNNER_ARCH", runner_arch),
    )?;
    println!("Qualified packaged runtime for {target}");
    Ok(())
}

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
        && let Some(version) = reference.strip_prefix("refs/tags/v")
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

/// Synchronize release metadata before compiling platform archives.
///
/// # Errors
/// Rejects non-stable versions or malformed package metadata.
pub fn prepare_version(root: &Path, version: &str) -> Result<()> {
    let parsed = semver::Version::parse(version)?;
    ensure!(
        parsed.pre.is_empty() && parsed.build.is_empty(),
        "stable release version required"
    );
    let manifest_path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)?;
    let metadata: toml::Value = toml::from_str(&manifest)?;
    let previous = metadata["package"]["version"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing package version"))?;
    let name = metadata["package"]["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing package name"))?;
    let lock_path = root.join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)?;
    let record = format!("name = \"{name}\"\nversion = \"{previous}\"");
    ensure!(
        lock.matches(&record).count() == 1,
        "expected one root package in Cargo.lock"
    );
    let declaration = format!("version = \"{previous}\"");
    ensure!(
        manifest.matches(&declaration).count() == 1,
        "expected one package version declaration"
    );
    fs::write(
        manifest_path,
        manifest.replacen(&declaration, &format!("version = \"{version}\""), 1),
    )?;
    fs::write(
        lock_path,
        lock.replacen(
            &record,
            &format!("name = \"{name}\"\nversion = \"{version}\""),
            1,
        ),
    )?;
    fs::write(root.join("runtime-version"), format!("{version}\n"))?;
    Ok(())
}
