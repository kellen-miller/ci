//! Release policy and lifecycle; JavaScript is only the semantic-release API bridge.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{command, github_output};

#[derive(Deserialize)]
struct CommitFilter {
    commits: Vec<Value>,
    cwd: PathBuf,
    paths: Vec<String>,
}

/// Normalize repository-relative release scope boundaries.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn scope_paths(value: &str) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let relative_prefix = Regex::new(r"^\./+")?;
    for path in value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let normalized = path.replace('\\', "/");
        ensure!(
            !normalized.starts_with('/'),
            "Release commit paths must be repository-relative: {path}"
        );
        let normalized = relative_prefix.replace(&normalized, "");
        let normalized = normalized.trim_end_matches('/');
        let normalized = if normalized.is_empty() {
            "."
        } else {
            normalized
        };

        ensure!(
            !normalized.starts_with('/')
                && !normalized.split('/').any(|part| part == "..")
                && !normalized.contains('\r'),
            "Release commit paths must be repository-relative: {path}"
        );
        paths.push(normalized.to_owned());
    }

    Ok(paths)
}

/// Filter semantic-release commit objects using actual changed files.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn filter_commits() -> Result<()> {
    let input: CommitFilter = serde_json::from_reader(io::stdin().lock())?;
    ensure!(
        !input.paths.is_empty(),
        "Path-scoped semantic-release requires at least one commit path"
    );
    let hash_pattern = Regex::new(r"^[0-9a-fA-F]{7,64}$")?;
    let mut selected = Vec::new();
    for commit in input.commits {
        let hash = commit["hash"].as_str().context("commit hash missing")?;
        ensure!(
            hash_pattern.is_match(hash),
            "Invalid commit hash from semantic-release: {hash}"
        );
        let files = command::capture_bytes(Command::new("git").current_dir(&input.cwd).args([
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-z",
            "-r",
            hash,
            "--",
        ]))?;
        let matches = files
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .any(|name| {
                let name = String::from_utf8_lossy(name).replace('\\', "/");
                input.paths.iter().any(|path| {
                    path == "." || name == *path || name.starts_with(&format!("{path}/"))
                })
            });
        if matches {
            selected.push(commit);
        }
    }

    serde_json::to_writer(io::stdout().lock(), &selected)?;
    Ok(())
}

/// Invoke the Node-native engine once, returning its structured result.
fn invoke_engine(
    directory: &Path,
    cwd: &Path,
    settings: &Value,
    dry_run: bool,
    paths: &[String],
) -> Result<Value> {
    let config = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(config.as_file(), settings)?;
    let result = tempfile::NamedTempFile::new()?;
    command::run(
        Command::new("node")
            .arg(directory.join("bridge.mjs"))
            .current_dir(cwd)
            .env("CI_RELEASE_SETTINGS", config.path())
            .env("CI_RELEASE_RESULT", result.path())
            .env("CI_RELEASE_DRY_RUN", dry_run.to_string())
            .env("CI_RELEASE_BINARY", env::current_exe()?)
            .env("CI_RELEASE_PATHS", serde_json::to_string(paths)?),
    )?;
    Ok(serde_json::from_slice(&fs::read(result.path())?)?)
}

/// Execute release orchestration with the existing action environment contract.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn run() -> Result<()> {
    let cwd = fs::canonicalize(env::var("WORKING_DIRECTORY").unwrap_or_else(|_| ".".into()))?;
    let dry_run = env::var("DRY_RUN").is_ok_and(|value| value == "true");
    let first_version = env::var("FIRST_RELEASE_VERSION").unwrap_or_default();
    if !first_version.is_empty() {
        semver::Version::parse(&first_version)
            .context("first-release-version must be a valid semver version")?;
    }

    let mut scope = env::var("SEMANTIC_RELEASE_COMMIT_PATHS").unwrap_or_default();
    let workspace = fs::canonicalize(env::var("GITHUB_WORKSPACE").unwrap_or_else(|_| ".".into()))?;
    if scope.trim().is_empty() && cwd != workspace {
        scope = command::capture(
            Command::new("git")
                .current_dir(&cwd)
                .args(["rev-parse", "--show-prefix"]),
        )?
        .trim()
        .to_owned();
    }

    let paths = scope_paths(&scope)?;
    let root =
        PathBuf::from(env::var("CI_ROOT").unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").into()));
    let directory = root.join(".github/actions/release/semantic-release");
    let extra = env::var("EXTRA_PLUGINS").unwrap_or_default();
    let extras: Vec<&str> = extra
        .split([',', '\n'])
        .map(str::trim)
        .filter(|plugin| !plugin.is_empty())
        .collect();
    let package = Regex::new(r"^(@[a-z0-9][\w.-]*/[a-z0-9][\w.-]*|[a-z0-9][\w.-]*)@[^\s/]+$")?;
    let tag = Regex::new(r"^[a-zA-Z][\w.-]*$")?;
    for spec in &extras {
        let version = spec
            .rsplit_once('@')
            .map(|(_, version)| version)
            .unwrap_or_default();
        ensure!(
            package.is_match(spec)
                && (semver::Version::parse(version).is_ok()
                    || semver::VersionReq::parse(version).is_ok()
                    || tag.is_match(version)),
            "Extra plugins must be public npm package@version specifications"
        );
    }

    if !extras.is_empty() {
        command::run(
            Command::new("npm")
                .arg("install")
                .arg("--prefix")
                .arg(&directory)
                .args([
                    "--no-save",
                    "--package-lock=false",
                    "--ignore-scripts",
                    "--no-audit",
                    "--no-fund",
                    "--registry=https://registry.npmjs.org",
                ])
                .args(&extras),
        )?;
    }

    let analyzer = json!({"preset":"conventionalcommits","releaseRules":[{"breaking":true,"release":if env::var("PRELAUNCH").is_ok_and(|value| value == "true") {"minor"} else {"major"}},{"type":"refactor","release":"patch"}]});
    let notes = json!({"preset":"conventionalcommits"});
    let mut plugins = if paths.is_empty() {
        vec![
            json!(["@semantic-release/commit-analyzer", analyzer]),
            json!(["@semantic-release/release-notes-generator", notes]),
        ]
    } else {
        vec![json!([directory.join("path-scope/bridge.mjs"), {"analyzer":analyzer,"notes":notes}])]
    };

    plugins.push(json!(["@semantic-release/github",{"successComment":false,"failComment":false,"releasedLabels":false}]));
    let branches = env::var("SEMANTIC_RELEASE_BRANCHES")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "main".into());
    let prefix = env::var("SEMANTIC_RELEASE_TAG_PREFIX")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "v".into());
    let config = env::var("CONFIG_FILE").unwrap_or_default();
    let settings = json!({"plugins":plugins,"branches":branches.split(',').map(str::trim).collect::<Vec<_>>(),"tagFormat":format!("{prefix}${{version}}"),"configFile":if config.is_empty() { None } else { Some(cwd.join(config)) },"extraPlugins":extras.iter().filter_map(|spec| spec.rsplit_once('@').map(|(name,_)|name)).collect::<Vec<_>>()});
    let mut result = invoke_engine(
        &directory,
        &cwd,
        &settings,
        dry_run || !first_version.is_empty(),
        &paths,
    )?;
    let mut strategy = if result.is_object() {
        "semantic"
    } else {
        "none"
    };

    if !first_version.is_empty() && result.is_object() {
        if result["lastRelease"]["version"]
            .as_str()
            .is_none_or(str::is_empty)
        {
            let tag = format!("{prefix}{first_version}");
            result["nextRelease"]["version"] = json!(first_version);
            result["nextRelease"]["gitTag"] = json!(tag);
            strategy = "initial";
            if !dry_run {
                let mut notes = tempfile::NamedTempFile::new()?;
                notes.write_all(
                    result["nextRelease"]["notes"]
                        .as_str()
                        .unwrap_or_default()
                        .as_bytes(),
                )?;
                command::run(
                    Command::new("gh")
                        .args([
                            "release",
                            "create",
                            &tag,
                            "--repo",
                            &env::var("GITHUB_REPOSITORY")?,
                            "--target",
                            result["nextRelease"]["gitHead"]
                                .as_str()
                                .context("release git head missing")?,
                            "--title",
                            &tag,
                            "--notes-file",
                        ])
                        .arg(notes.path())
                        .current_dir(&cwd),
                )?;
            }
        } else {
            result = invoke_engine(&directory, &cwd, &settings, dry_run, &paths)?;
        }
    }

    let version = result["nextRelease"]["version"]
        .as_str()
        .unwrap_or_default();
    let major_minor = if version.is_empty() {
        String::new()
    } else {
        let version = semver::Version::parse(version)?;
        format!("{}.{}", version.major, version.minor)
    };

    for (key, value) in [
        (
            "release_published",
            (result.is_object() && !dry_run).to_string(),
        ),
        ("version", version.to_owned()),
        ("major_minor", major_minor),
        (
            "git_tag",
            result["nextRelease"]["gitTag"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ),
        ("release_strategy", strategy.to_owned()),
    ] {
        github_output::append(key, &value)?;
    }

    Ok(())
}
