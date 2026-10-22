//! Custom orchestration used by reusable workflows.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use anyhow::{Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{command, github_output};

#[derive(Serialize)]
struct Platform {
    platform: String,
    suffix: String,
    runner_label: String,
}

#[derive(Serialize)]
struct Matrix {
    include: Vec<Platform>,
}

/// Execute one workflow operation with arguments passed as data.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn run(operation: &str) -> Result<()> {
    match operation {
        "pages-build" => {
            let mut build = command::script(env::var("PM")?);
            build.args(["run", &env::var("SCRIPT")?]);
            let assignments = env::var("BUILD_ENV").unwrap_or_default();
            for assignment in assignments.lines().filter(|line| !line.is_empty()) {
                let (name, value) = assignment
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("Invalid build environment assignment"))?;
                ensure!(
                    !name.is_empty()
                        && name.bytes().enumerate().all(|(index, byte)| byte == b'_'
                            || byte.is_ascii_alphabetic()
                            || (index > 0 && byte.is_ascii_digit())),
                    "Invalid build environment assignment"
                );
                build.env(name, value);
            }

            command::run(&mut build)?;
        }

        "playwright-install" | "playwright-deps" => {
            let browsers = env::var("BROWSERS").unwrap_or_default();
            let mut install = command::script("./node_modules/.bin/playwright");
            if operation == "playwright-install" {
                install.args(["install", "--with-deps"]);
            } else {
                install.arg("install-deps");
            }

            command::run(install.args(browsers.split_whitespace()))?;
        }

        "node-checks" => {
            let manager = env::var("PM")?;
            let scripts = ["FORMAT_SCRIPT", "CHECK_SCRIPT", "LINT_SCRIPT"]
                .map(|name| env::var(name).unwrap_or_default());
            ensure!(
                scripts.iter().any(|script| !script.is_empty()),
                "At least one format, check or lint script is required"
            );
            for script in scripts.iter().filter(|script| !script.is_empty()) {
                command::run(command::script(&manager).args(["run", script]))?;
            }
        }

        "renovate-changes" => {
            let mut validate = true;
            if env::var("EVENT_NAME")? == "pull_request" {
                let diff = command::capture_bytes(Command::new("git").args([
                    "diff",
                    "--name-only",
                    "--no-renames",
                    "-z",
                    &format!("{}...{}", env::var("BASE_SHA")?, env::var("HEAD_SHA")?),
                    "--",
                    &format!(":(literal){}", env::var("CONFIG_FILE")?),
                    ":(literal)default.json",
                    ".github/workflows/",
                ]))?;
                validate = !diff.is_empty();
            }

            github_output::append("validate", if validate { "true" } else { "false" })?;
        }

        "renovate-validate" => {
            let mut cmd = command::script("npx");
            cmd.args([
                "--yes",
                "--package",
                &format!("renovate@{}", env::var("RENOVATE_VERSION")?),
                "--",
                "renovate-config-validator",
            ]);
            if env::var("GLOBAL_CONFIG")? != "true" {
                cmd.arg("--no-global");
            }

            command::run(cmd.arg(env::var("CONFIG_FILE")?))?;
        }

        "platform-matrix" => {
            let platforms = env::var("PLATFORMS")?;
            let requested: Vec<&str> = if platforms.trim().is_empty() {
                vec![""]
            } else {
                platforms
                    .split(',')
                    .map(str::trim)
                    .filter(|platform| !platform.is_empty())
                    .collect()
            };

            let mut include: Vec<Platform> = Vec::new();
            for platform in requested {
                if include.iter().any(|item| item.platform == platform) {
                    continue;
                }

                let suffix = platform_suffix(platform);
                ensure!(
                    !include.iter().any(|item| item.suffix == suffix),
                    "platform artifact names collide"
                );
                include.push(Platform {
                    platform: platform.into(),
                    suffix,
                    runner_label: if platform == "linux/arm64" {
                        "ubuntu-24.04-arm"
                    } else {
                        "ubuntu-latest"
                    }
                    .into(),
                });
            }

            ensure!(!include.is_empty(), "no build platforms selected");
            github_output::append("matrix", &serde_json::to_string(&Matrix { include })?)?;
        }

        "digest-artifact" => {
            let digest = env::var("DIGEST")?;
            ensure!(valid_digest(&digest), "Invalid image digest: {digest}");
            let directory = PathBuf::from(env::var("RUNNER_TEMP")?).join("digests");
            fs::create_dir_all(&directory)?;
            fs::write(directory.join(&digest[7..]), [])?;
        }

        "docker-manifest" => {
            let image = env::var("IMAGE")?;
            let mut files =
                fs::read_dir(env::var("DIGEST_DIR")?)?.collect::<std::io::Result<Vec<_>>>()?;
            files.sort_by_key(|file| file.file_name());
            let mut digests = Vec::new();
            for file in files {
                ensure!(file.file_type()?.is_file(), "invalid digest artifact");
                let digest = format!(
                    "sha256:{}",
                    file.file_name()
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("invalid digest filename"))?
                );
                ensure!(valid_digest(&digest), "invalid digest artifact: {digest}");
                digests.push(format!("{image}@{digest}"));
            }

            ensure!(!digests.is_empty(), "No digest artifacts were downloaded");
            let tags = env::var("TAGS")?;
            let tags: Vec<&str> = tags.lines().filter(|tag| !tag.is_empty()).collect();
            ensure!(!tags.is_empty(), "No Docker tags were resolved");
            let mut create = Command::new("docker");
            create.args(["buildx", "imagetools", "create"]);
            for tag in &tags {
                create.arg("--tag").arg(tag);
            }

            command::run(create.args(&digests))?;
            let manifest = command::capture_bytes(Command::new("docker").args([
                "buildx",
                "imagetools",
                "inspect",
                "--raw",
                tags[0],
            ]))?;
            github_output::append(
                "digest",
                &format!("sha256:{}", hex::encode(Sha256::digest(&manifest))),
            )?;
        }

        "helm-package" => {
            let chart = env::var("CHART_PATH")?;
            let version = env::var("CHART_VERSION")?;
            let destination = PathBuf::from(env::var("RUNNER_TEMP")?).join("chart-out");
            command::run(Command::new("helm").args(["dependency", "build", &chart]))?;
            fs::create_dir_all(&destination)?;
            command::run(
                Command::new("helm")
                    .args([
                        "package",
                        &chart,
                        "--version",
                        &version,
                        "--app-version",
                        &version,
                        "--destination",
                    ])
                    .arg(destination),
            )?;
        }

        "helm-push" => {
            let registry = format!(
                "oci://{}-docker.pkg.dev/{}/{}",
                env::var("REGION")?,
                env::var("PROJECT_ID")?,
                env::var("REPO_NAME")?
            );
            let directory = PathBuf::from(env::var("RUNNER_TEMP")?).join("chart-out");
            let mut files = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
            files.sort_by_key(|file| file.file_name());
            let mut count = 0;
            for file in files {
                if file
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "tgz")
                {
                    command::run(
                        Command::new("helm")
                            .arg("push")
                            .arg(file.path())
                            .arg(&registry),
                    )?;
                    count += 1;
                }
            }

            ensure!(count > 0, "No packaged charts found");
        }

        "provenance" => {
            let repository = env::var("REPOSITORY")?;
            let reference = env::var("REF")?;
            let sha = env::var("SHA")?;
            let workflow = env::var("WORKFLOW_REF")?;
            let workflow_sha = env::var("WORKFLOW_SHA")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| sha.clone());
            let predicate = serde_json::json!({"buildDefinition":{"buildType":env::var("BUILD_TYPE")?,"externalParameters":{"repository":repository,"ref":reference,"workflowRef":workflow},"internalParameters":{},"resolvedDependencies":[{"uri":format!("git+{repository}@{reference}"),"digest":{"gitCommit":sha}},{"uri":workflow,"digest":{"gitCommit":workflow_sha}}]},"runDetails":{"builder":{"id":"https://github.com/actions/runner"},"metadata":{"invocationId":env::var("INVOCATION_ID")?}}});
            fs::write(
                "slsa-provenance.json",
                format!("{}\n", serde_json::to_string_pretty(&predicate)?),
            )?;
        }

        _ => bail!("Unknown workflow operation: {operation}"),
    }

    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn platform_suffix(platform: &str) -> String {
    if platform.is_empty() {
        return "default".into();
    }

    platform
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "_.-".contains(character) {
                character
            } else {
                '-'
            }
        })
        .collect()
}
