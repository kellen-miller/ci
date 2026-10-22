//! Node package manager detection and locked dependency installation.

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::{command, github_output};

pub fn run(operation: &str) -> Result<()> {
    match operation {
        "detect" => detect(),
        "install" => install(),
        _ => bail!("unknown node operation: {operation}"),
    }
}

fn detect() -> Result<()> {
    let package: Value =
        serde_json::from_slice(&fs::read("package.json").context("read package.json")?)?;
    let candidates: [(&str, &[&str]); 4] = [
        ("pnpm", &["pnpm-lock.yaml"]),
        ("npm", &["package-lock.json"]),
        ("yarn", &["yarn.lock"]),
        ("bun", &["bun.lock", "bun.lockb"]),
    ];
    let manager_input = env::var("INPUT_PM").unwrap_or_default();
    let found = candidates
        .iter()
        .map(|(name, files)| {
            (
                *name,
                files
                    .iter()
                    .copied()
                    .filter(|path| Path::new(path).is_file())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let manager = if manager_input.is_empty() {
        let detected = found
            .iter()
            .filter(|(_, files)| !files.is_empty())
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        ensure!(
            detected.len() == 1,
            "Expected one package manager lockfile; set package-manager explicitly"
        );
        detected[0]
    } else {
        manager_input.as_str()
    };
    let files = found
        .iter()
        .find(|(name, _)| *name == manager)
        .map(|(_, files)| files);
    ensure!(
        files.is_some_and(|files| !files.is_empty()),
        "Unsupported package manager or missing lockfile: {manager}"
    );
    let files = files.expect("validated package manager");
    ensure!(
        files.len() == 1,
        "Multiple {manager} lockfiles found; keep one"
    );

    let mut version = env::var("INPUT_NODE").unwrap_or_default();
    if version.is_empty() {
        for path in [".node-version", ".nvmrc"] {
            if Path::new(path).is_file() {
                version = fs::read_to_string(path)?.trim().to_owned();
                break;
            }
        }
        if version.is_empty() {
            version = package["engines"]["node"]
                .as_str()
                .unwrap_or("24")
                .to_owned();
        }
    }
    let workdir = env::var("INPUT_WD")?;
    let lockfile = Path::new(&workdir)
        .join(files[0])
        .to_string_lossy()
        .replace('\\', "/");
    ensure!(
        !manager.contains(['\n', '\r'])
            && !lockfile.contains(['\n', '\r'])
            && !version.contains(['\n', '\r']),
        "Node setup outputs must be single-line values"
    );
    github_output::append("package-manager", manager)?;
    github_output::append("lockfile", &lockfile)?;
    github_output::append("node-version", &version)
}

fn install() -> Result<()> {
    let manager = env::var("PM")?;
    let mut args = match manager.as_str() {
        "pnpm" => vec!["install".to_owned(), "--frozen-lockfile".to_owned()],
        "npm" => vec!["ci".to_owned()],
        "yarn" => {
            let version = command::capture(Command::new("yarn").arg("--version"))?;
            let major = version
                .trim()
                .split('.')
                .next()
                .unwrap_or("0")
                .parse::<u64>()
                .context("parse yarn major version")?;
            if major >= 2 {
                vec!["install".to_owned(), "--immutable".to_owned()]
            } else {
                vec!["install".to_owned(), "--frozen-lockfile".to_owned()]
            }
        }
        "bun" => vec!["install".to_owned(), "--frozen-lockfile".to_owned()],
        _ => bail!("Unsupported package manager: {manager}"),
    };
    if manager == "pnpm" {
        for (environment, flag) in [
            ("PNPM_MINIMUM_RELEASE_AGE", "--config.minimum-release-age="),
            ("PNPM_STRICT_DEP_BUILDS", "--config.strict-dep-builds="),
        ] {
            if let Ok(value) = env::var(environment)
                && !value.is_empty()
            {
                args.push(format!("{flag}{value}"));
            }
        }
    }
    if let Ok(extra) = env::var("EXTRA_ARGS") {
        args.extend(extra.split_whitespace().map(str::to_owned));
    }
    command::run(Command::new(manager).args(args))
}
