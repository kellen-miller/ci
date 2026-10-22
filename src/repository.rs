//! Repository validation and immutable action-reference maintenance.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, ensure};
use regex::Regex;

use crate::{command, workflow_security};

/// Locate bundled source/configuration without depending on the consumer cwd.
pub fn root() -> PathBuf {
    env::var_os("CI_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// Reject a released executable that does not implement the selected action sources.
///
/// # Errors
/// Returns an error when source files are missing or differ from the build inputs.
pub fn verify_runtime_source() -> Result<()> {
    ensure!(
        crate::source_identity::digest(&root())? == env!("CI_SOURCE_DIGEST"),
        "Runtime binary does not match action sources; publish a matching runtime version before upgrading action pins"
    );
    Ok(())
}

/// Run the same selected validation phases locally and on hosted CI.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn check() -> Result<()> {
    let root = root();
    let selected: Option<BTreeMap<String, bool>> = serde_json::from_str(
        &env::var("CI_CHANGED_GROUPS")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "null".into()),
    )?;
    let enabled = |name: &str| {
        selected
            .as_ref()
            .is_none_or(|groups| groups.get(name).copied().unwrap_or(true))
    };

    let run = |name: &str, executable: &str, args: &[&str]| -> Result<()> {
        if enabled(name) {
            println!("Running {executable} {}", args.join(" "));
            command::run(command::script(executable).args(args).current_dir(&root))?;
        } else {
            println!("Skipping {name}: inputs unchanged");
        }

        Ok(())
    };

    run("yaml", "yamllint", &["--strict", "."])?;
    run("workflows", "actionlint", &[])?;
    run(
        "go_config",
        "golangci-lint",
        &["config", "verify", "--config", "config/golangci.yaml"],
    )?;
    run(
        "renovate",
        "npx",
        &[
            "--yes",
            "--package",
            "renovate@44.145.3",
            "--",
            "renovate-config-validator",
            "--strict",
            "--no-global",
            "default.json",
            "renovate.json",
        ],
    )?;
    if enabled("workflows") {
        workflow_security::run(&root, false, false)?;
    }

    if enabled("shell") {
        let files = command::capture_bytes(Command::new("git").current_dir(&root).args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".github",
            "scripts",
        ]))?;
        let paths = files
            .split(|byte| *byte == 0)
            .filter(|file| file.ends_with(b".sh"))
            .map(|file| String::from_utf8(file.to_vec()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !paths.is_empty() {
            command::run(Command::new("shellcheck").args(paths).current_dir(&root))?;
        }
    }

    if enabled("rust") {
        command::run(
            Command::new(env::current_exe()?)
                .arg("rust-check")
                .current_dir(&root)
                .env("CI_ROOT", &root)
                .env("WORKING_DIRECTORY", &root)
                .env("CHECKS", "fmt clippy test")
                .env("SETUP_TOOLCHAIN", "false")
                .env("POLICY_DIRECTORY", "")
                .env("ALL_FEATURES", "true")
                .env("BUILD_PROFILE", "dev"),
        )?;
    }

    if enabled("release") {
        command::run(
            command::script("npm")
                .args([
                    "ci",
                    "--prefix",
                    ".github/actions/release/semantic-release",
                    "--ignore-scripts",
                    "--no-audit",
                    "--no-fund",
                ])
                .current_dir(&root),
        )?;
        command::run(
            Command::new("cargo")
                .args(["test", "--locked", "--test", "release", "--", "--ignored"])
                .current_dir(&root),
        )?;
    }

    Ok(())
}

/// Pin workflows only after every runtime implementation input is committed.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn pin_actions(commit: &str) -> Result<()> {
    let root = root();
    let commit = command::capture(Command::new("git").current_dir(&root).args([
        "rev-parse",
        "--verify",
        &format!("{commit}^{{commit}}"),
    ]))?
    .trim()
    .to_owned();
    let inputs = [
        ".github/actions",
        "src",
        "Cargo.toml",
        "Cargo.lock",
        "config",
        "runtime-version",
        "build.rs",
    ];
    let dirty = command::capture_bytes(
        Command::new("git")
            .current_dir(&root)
            .arg("diff")
            .arg(&commit)
            .arg("--")
            .args(inputs),
    )?;
    let untracked = command::capture_bytes(
        Command::new("git")
            .current_dir(&root)
            .args(["ls-files", "--others", "--exclude-standard", "--"])
            .args(inputs),
    )?;
    ensure!(
        dirty.is_empty() && untracked.is_empty(),
        "Commit action, Rust and policy changes before pinning workflows"
    );
    for required in [
        "build.rs",
        "Cargo.toml",
        "Cargo.lock",
        "src/main.rs",
        "runtime-version",
        ".github/actions/run.sh",
    ] {
        command::run(Command::new("git").current_dir(&root).args([
            "cat-file",
            "-e",
            &format!("{commit}:{required}"),
        ]))?;
    }

    let reference = Regex::new(r"kellen-miller/ci/(\.github/actions/[^@\s]+)@[0-9a-f]{40}")?;
    let mut changes = Vec::new();
    for entry in fs::read_dir(root.join(".github/workflows"))? {
        let path = entry?.path();
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yaml" | "yml")
        ) {
            continue;
        }

        let text = fs::read_to_string(&path)?;
        for found in reference.captures_iter(&text) {
            command::run(Command::new("git").current_dir(&root).args([
                "cat-file",
                "-e",
                &format!("{commit}:{}/action.yaml", &found[1]),
            ]))?;
        }

        let updated = reference
            .replace_all(&text, format!("kellen-miller/ci/${{1}}@{commit}"))
            .into_owned();
        changes.push((path, updated));
    }

    for (path, text) in changes {
        fs::write(path, text)?;
    }

    println!("Pinned shared actions to {commit}");
    Ok(())
}
