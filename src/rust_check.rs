//! Execute the shared Rust checks in the requested order with bundled policy.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::command::{capture, run as run_child};
use crate::rust_sources;

#[derive(Deserialize)]
pub(crate) struct Metadata {
    pub workspace_members: Vec<String>,
    pub packages: Vec<Package>,
    pub target_directory: Option<PathBuf>,
}

#[derive(Deserialize)]
pub(crate) struct Package {
    pub id: String,
    pub name: Option<String>,
    pub manifest_path: Option<PathBuf>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub targets: Vec<Target>,
}

impl Package {
    pub(crate) fn generated(&self) -> bool {
        self.metadata.get("ci").and_then(|ci| ci.get("generated"))
            == Some(&serde_json::Value::Bool(true))
    }
}

#[derive(Deserialize)]
pub(crate) struct Target {
    doctest: Option<bool>,
}

/// Run checks using the same environment inputs as the composite action.
///
/// # Errors
/// Returns input, policy, metadata, source ownership, or child process failures.
pub fn run() -> Result<()> {
    let setup = env::var("SETUP_TOOLCHAIN").unwrap_or_else(|_| "true".into());
    let directory = env::var("WORKING_DIRECTORY").unwrap_or_else(|_| ".".into());
    let selected = env::var("CHECKS").unwrap_or_else(|_| "fmt clippy test build".into());
    let all_features = env::var("ALL_FEATURES").unwrap_or_else(|_| "true".into());
    let profile = env::var("BUILD_PROFILE").unwrap_or_else(|_| "dev".into());
    let policy = env::var("POLICY_DIRECTORY").unwrap_or_default();
    for value in [
        &setup,
        &directory,
        &selected,
        &all_features,
        &profile,
        &policy,
    ] {
        ensure!(
            !value.contains(['\n', '\r']),
            "Rust check inputs must be single-line values"
        );
    }

    ensure!(!directory.is_empty(), "working-directory must not be empty");
    ensure!(
        matches!(setup.as_str(), "true" | "false"),
        "setup-toolchain must be true or false"
    );
    ensure!(
        matches!(all_features.as_str(), "true" | "false"),
        "all-features must be true or false"
    );
    ensure!(
        matches!(profile.as_str(), "dev" | "release"),
        "build-profile must be dev or release"
    );
    // Bash's explicit space-only IFS rejects tabs inside a check name.
    let checks: Vec<&str> = selected
        .split(' ')
        .filter(|value| !value.is_empty())
        .collect();
    ensure!(!checks.is_empty(), "checks must contain at least one check");
    let mut seen = HashSet::new();
    for check in &checks {
        ensure!(
            matches!(*check, "fmt" | "clippy" | "test" | "build"),
            "checks must contain only fmt, clippy, test or build"
        );
        ensure!(seen.insert(*check), "checks must not contain duplicates");
    }

    let directory = dunce::canonicalize(directory).context("resolve Rust working-directory")?;
    ensure!(
        directory.is_dir(),
        "Rust working-directory must be a directory"
    );
    let mut policy_directory = PathBuf::new();
    let mut lint_args = Vec::new();
    if seen.contains("fmt") || seen.contains("clippy") {
        policy_directory = dunce::canonicalize(if policy.is_empty() {
            env::var_os("CI_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
                .join("config/rust")
        } else {
            directory.join(policy)
        })
        .context("resolve Rust policy-directory")?;
        if seen.contains("fmt") {
            ensure!(
                policy_directory.join("rustfmt.toml").is_file(),
                "Missing Rust formatting policy: {}",
                policy_directory.join("rustfmt.toml").display()
            );
        }

        if seen.contains("clippy") {
            lint_args = read_lint_policy(&policy_directory)?;
        }
    }

    let features = (all_features == "true").then_some("--all-features");
    let mut workspace = vec!["--locked", "--workspace"];
    workspace.extend(features);

    for check in checks {
        let mut cargo = Command::new("cargo");
        cargo.current_dir(&directory).arg(check);
        match check {
            "fmt" => {
                cargo
                    .args(["--all", "--", "--check", "--config-path"])
                    .arg(policy_directory.join("rustfmt.toml"));
            }

            "clippy" => {
                let mut command = Command::new("cargo");
                command.current_dir(&directory).args([
                    "metadata",
                    "--locked",
                    "--format-version",
                    "1",
                ]);
                command.args(features);

                let metadata: Metadata = serde_json::from_str(&capture(&mut command)?)
                    .context("Invalid Cargo workspace metadata for Rust file-length check")?;
                rust_sources::check(&metadata, &directory)?;
                let members: Vec<&Package> = metadata
                    .packages
                    .iter()
                    .filter(|package| metadata.workspace_members.contains(&package.id))
                    .collect();
                let generated: Vec<&Package> = members
                    .iter()
                    .copied()
                    .filter(|package| package.generated())
                    .collect();
                if members.len() == generated.len() {
                    println!(
                        "Skipping Clippy: all workspace packages declare package.metadata.ci.generated = true"
                    );
                    continue;
                }

                cargo.args(&workspace);
                if !generated.is_empty() {
                    cargo.arg("--no-deps");
                }

                for package in generated {
                    cargo.arg("--exclude").arg(
                        package
                            .name
                            .as_deref()
                            .context("Cargo workspace package name is missing")?,
                    );
                }

                cargo
                    .args(["--all-targets", "--"])
                    .args(&lint_args)
                    .args(["-D", "warnings"])
                    .env("CLIPPY_CONF_DIR", &policy_directory);
            }

            "test" => {
                cargo.args(&workspace).arg("--all-targets");
                run_child(&mut cargo)?;
                let mut command = Command::new("cargo");
                command.current_dir(&directory).args([
                    "metadata",
                    "--locked",
                    "--no-deps",
                    "--format-version",
                    "1",
                ]);
                command.args(features);

                let metadata: Metadata = serde_json::from_str(&capture(&mut command)?)
                    .context("Invalid Cargo workspace metadata for Rust doctests")?;
                let mut has_doctests = false;
                for target in metadata
                    .packages
                    .iter()
                    .filter(|package| metadata.workspace_members.contains(&package.id))
                    .flat_map(|package| &package.targets)
                {
                    has_doctests |= target
                        .doctest
                        .context("Cargo target doctest metadata is missing")?;
                }

                if has_doctests {
                    run_child(
                        Command::new("cargo")
                            .current_dir(&directory)
                            .arg("test")
                            .args(&workspace)
                            .arg("--doc"),
                    )?;
                }

                continue;
            }

            "build" => {
                cargo
                    .args(&workspace)
                    .args(["--all-targets", "--profile", &profile]);
            }

            _ => unreachable!("validated check"),
        }

        run_child(&mut cargo)?;
    }

    Ok(())
}

// Normalize and validate the entire policy before executing any selected check.
fn read_lint_policy(directory: &Path) -> Result<Vec<String>> {
    ensure!(
        directory.join("clippy.toml").is_file(),
        "Missing Clippy configuration: {}",
        directory.join("clippy.toml").display()
    );
    let content =
        fs::read_to_string(directory.join("lints.toml")).context("read Rust lint policy")?;
    let tables: toml::Table = toml::from_str(&content).context("parse Rust lint policy")?;
    ensure!(
        tables.len() == 2 && tables.contains_key("rust") && tables.contains_key("clippy"),
        "Rust policy must contain nonempty rust and clippy lint tables"
    );
    let mut arguments = Vec::new();
    for (namespace, value) in tables {
        let lints = value
            .as_table()
            .context("Rust policy lint entries must be tables")?;
        ensure!(
            !lints.is_empty(),
            "Rust policy must contain nonempty rust and clippy lint tables"
        );
        for (name, value) in lints {
            let flag = match value.as_str() {
                Some("allow") => "-A",
                Some("warn") => "-W",
                Some("deny") => "-D",
                Some("forbid") => "-F",
                _ => bail!("Invalid Rust lint policy entry: {namespace}.{name}"),
            };

            let mut letters = name.chars();
            ensure!(
                letters
                    .next()
                    .is_some_and(|letter| letter == '_' || letter.is_alphabetic())
                    && letters.all(|letter| letter == '_' || letter.is_alphanumeric()),
                "Invalid Rust lint policy entry: {namespace}.{name}"
            );
            arguments.push(flag.to_string());
            arguments.push(if namespace == "clippy" {
                format!("clippy::{name}")
            } else {
                name.clone()
            });
        }
    }

    Ok(arguments)
}
