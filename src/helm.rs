//! Helm chart rendering, schema validation, and verified kubeconform install.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use flate2::read::GzDecoder;
use regex::Regex;
use sha2::Digest;

use crate::{command, github_output};

/// Run one Helm or kubeconform operation.
///
/// # Errors
/// Returns an error for unsupported operations, invalid inputs, failed child processes, network failures, or filesystem errors.
pub fn run(operation: &str) -> Result<()> {
    match operation {
        "lint" => lint(),
        "install-kubeconform" => install_kubeconform(),
        _ => bail!("unknown helm operation: {operation}"),
    }
}

fn lint() -> Result<()> {
    let charts_root =
        PathBuf::from(env::var("CHARTS_ROOT").context("Set CHARTS_ROOT")?).canonicalize()?;
    let rendered = if let Some(directory) = env::var_os("RENDERED_DIR") {
        PathBuf::from(directory)
    } else {
        tempfile::Builder::new()
            .prefix("helm-rendered-")
            .tempdir_in(env::var("RUNNER_TEMP")?)?
            .keep()
    };
    fs::create_dir_all(&rendered)?;
    let mut charts = Vec::new();
    for entry in walkdir::WalkDir::new(&charts_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !matches!(
                    entry.file_name().to_string_lossy().as_ref(),
                    "charts" | "templates"
                )
        })
    {
        let entry = entry?;
        if entry.file_type().is_dir()
            && entry.depth() > 0
            && matches!(
                entry.file_name().to_string_lossy().as_ref(),
                "charts" | "templates"
            )
        {
            continue;
        }
        if entry.file_type().is_file() && entry.file_name() == "Chart.yaml" {
            charts.push(
                entry
                    .path()
                    .parent()
                    .context("chart file has no parent")?
                    .to_path_buf(),
            );
        }
    }
    charts.sort();
    ensure!(
        !charts.is_empty(),
        "No charts found under {}",
        charts_root.display()
    );

    let mut schema_args = vec!["-strict".to_owned(), "-summary".to_owned()];
    if env::var("IGNORE_MISSING_SCHEMAS").unwrap_or_else(|_| "false".to_owned()) == "true" {
        schema_args.push("-ignore-missing-schemas".to_owned());
    }
    let schema_locations = env::var("SCHEMA_LOCATIONS")
        .ok()
        .filter(|locations| !locations.is_empty())
        .unwrap_or_else(|| "default".to_owned());
    for location in schema_locations.lines().filter(|line| !line.is_empty()) {
        schema_args.extend(["-schema-location".to_owned(), location.to_owned()]);
    }
    if let Ok(version) = env::var("KUBERNETES_VERSION")
        && !version.is_empty()
    {
        schema_args.extend(["-kubernetes-version".to_owned(), version]);
    }
    if let Ok(cache) = env::var("KUBECONFORM_CACHE")
        && !cache.is_empty()
    {
        fs::create_dir_all(&cache)?;
        schema_args.extend(["-cache".to_owned(), cache]);
    }

    let values_pattern = glob::Pattern::new(
        &env::var("VALUES_PATTERN").unwrap_or_else(|_| "values-*.yaml".to_owned()),
    )?;
    let validate_schemas =
        env::var("VALIDATE_SCHEMAS").unwrap_or_else(|_| "true".to_owned()) == "true";
    let mut index = 0usize;
    for chart in charts {
        println!("Checking {}", chart.display());
        let metadata = fs::read_to_string(chart.join("Chart.yaml"))?;
        if chart.join("Chart.lock").is_file()
            || metadata
                .lines()
                .any(|line| line.starts_with("dependencies:"))
        {
            command::run(
                Command::new("helm")
                    .args(["dependency", "build"])
                    .arg(&chart),
            )?;
        }
        let mut values = Vec::new();
        for entry in fs::read_dir(&chart)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && values_pattern.matches_path(Path::new(&entry.file_name()))
            {
                values.push(entry.path());
            }
        }
        values.sort();
        let variants = std::iter::once(None)
            .chain(values.iter().map(Some))
            .collect::<Vec<_>>();
        for values_file in variants {
            let mut lint = Command::new("helm");
            lint.arg("lint").arg(&chart);
            if let Some(values_file) = values_file {
                lint.args(["-f"]).arg(values_file);
            }
            command::run(&mut lint)?;
            let mut template = Command::new("helm");
            template.arg("template").arg("lint").arg(&chart);
            if let Some(values_file) = values_file {
                template.args(["-f"]).arg(values_file);
            }
            let output = command::capture_bytes(&mut template)?;
            let rendered_file = rendered.join(format!("{index}.yaml"));
            fs::write(&rendered_file, output)?;
            if validate_schemas {
                command::run(
                    Command::new("kubeconform")
                        .args(&schema_args)
                        .arg(&rendered_file),
                )?;
            }
            index += 1;
        }
    }
    github_output::append("directory", &rendered.to_string_lossy())
}

fn install_kubeconform() -> Result<()> {
    let version = env::var("KUBECONFORM_VERSION")?;
    ensure!(
        Regex::new(r"^v[0-9]+\.[0-9]+\.[0-9]+$")?.is_match(&version),
        "Expected an explicit kubeconform release such as v0.8.0"
    );
    let platform = match env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        _ => bail!("Unsupported operating system"),
    };
    let architecture = match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => bail!("Unsupported architecture"),
    };
    let archive_name = format!("kubeconform-{platform}-{architecture}.tar.gz");
    let base = format!("https://github.com/yannh/kubeconform/releases/download/{version}");
    let client = reqwest::blocking::Client::new();
    let archive = client
        .get(format!("{base}/{archive_name}"))
        .send()?
        .error_for_status()?
        .bytes()?;
    let checksums = client
        .get(format!("{base}/CHECKSUMS"))
        .send()?
        .error_for_status()?
        .text()?;
    verify_archive_checksum(&checksums, &archive_name, &archive)?;

    let install_directory = env::var_os("KUBECONFORM_INSTALL_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("RUNNER_TEMP")
                .map(PathBuf::from)
                .map(|path| path.join("kubeconform-bin"))
        })
        .context("RUNNER_TEMP is required when KUBECONFORM_INSTALL_DIR is unset")?;
    fs::create_dir_all(&install_directory)?;
    let mut tar = tar::Archive::new(GzDecoder::new(archive.as_ref()));
    let mut found = false;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new("kubeconform") {
            let destination = install_directory.join("kubeconform");
            let mut binary = fs::File::create(&destination)?;
            std::io::copy(&mut entry, &mut binary)?;
            set_executable_permissions(&destination)?;
            found = true;
            break;
        }
    }
    ensure!(found, "kubeconform binary missing from verified archive");
    if let Some(path) = env::var_os("GITHUB_PATH") {
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
            "{}",
            install_directory.display()
        )?;
    }
    command::run(Command::new(install_directory.join("kubeconform")).arg("-v"))
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_permissions(path: &Path) -> Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn expected_checksum<'a>(checksums: &'a str, archive_name: &str) -> Option<&'a str> {
    checksums.lines().find_map(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        (fields.len() == 2 && fields[1].trim_start_matches('*') == archive_name)
            .then_some(fields[0])
    })
}

fn verify_archive_checksum(checksums: &str, archive_name: &str, archive: &[u8]) -> Result<()> {
    let actual = hex::encode(sha2::Sha256::digest(archive));
    ensure!(
        expected_checksum(checksums, archive_name).is_some_and(|expected| expected == actual),
        "Checksum verification failed for {archive_name}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{expected_checksum, verify_archive_checksum};
    use sha2::{Digest, Sha256};

    #[test]
    fn checksum_selects_only_the_named_archive() {
        let checksums = "abcd  other.tar.gz\nef01 *kubeconform-linux-amd64.tar.gz\n";
        assert_eq!(
            expected_checksum(checksums, "kubeconform-linux-amd64.tar.gz"),
            Some("ef01")
        );
        assert_eq!(expected_checksum(checksums, "missing.tar.gz"), None);
    }

    #[test]
    fn archive_checksum_must_match_before_install() {
        let archive = b"verified fixture";
        let checksum = hex::encode(Sha256::digest(archive));
        let checksums = format!("{checksum}  kubeconform-linux-amd64.tar.gz\n");
        assert!(
            verify_archive_checksum(&checksums, "kubeconform-linux-amd64.tar.gz", archive).is_ok()
        );
        assert!(
            verify_archive_checksum(
                "0000  kubeconform-linux-amd64.tar.gz",
                "kubeconform-linux-amd64.tar.gz",
                archive
            )
            .is_err()
        );
        assert!(
            verify_archive_checksum(&checksums, "kubeconform-darwin-arm64.tar.gz", archive)
                .is_err()
        );
    }
}
