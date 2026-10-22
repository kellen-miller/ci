//! Structured workflow reference, trigger, and credential policy.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_yaml_ng::Value;
use walkdir::WalkDir;

/// Validate all action/workflow YAML under root, with explicit policy opt-ins.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn run(root: &Path, allow_target: bool, allow_inherit: bool) -> Result<()> {
    let mut files = Vec::new();
    for directory in [root.join(".github/workflows"), root.join(".github/actions")] {
        if !directory.exists() {
            continue;
        }

        for entry in WalkDir::new(directory).into_iter().filter_entry(|entry| {
            !["node_modules", ".venv", ".git"]
                .contains(&entry.file_name().to_string_lossy().as_ref())
        }) {
            let entry = entry?;
            // Resolve YAML file links while WalkDir leaves directory links untraversed.
            if matches!(
                entry.path().extension().and_then(|value| value.to_str()),
                Some("yaml" | "yml")
            ) && std::fs::metadata(entry.path())
                .with_context(|| format!("inspect workflow YAML: {}", entry.path().display()))?
                .is_file()
            {
                files.push(entry.into_path());
            }
        }
    }

    ensure!(!files.is_empty(), "no workflow or action YAML files found");
    files.sort();
    let sha = Regex::new(r"@[0-9a-f]{40}$")?;
    let digest = Regex::new(r"@sha256:[0-9a-f]{64}$")?;
    let mut errors = Vec::new();
    for file in &files {
        let location = file.strip_prefix(root)?.display().to_string();
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("read workflow YAML: {}", file.display()))?;
        let document: Value = match serde_yaml_ng::from_str(&text) {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("{location}: invalid YAML: {error}"));
                continue;
            }
        };

        if !document.is_mapping() {
            errors.push(format!("{location}: expected a YAML mapping"));
            continue;
        }

        if !allow_target {
            let event = &document["on"];
            let privileged = event.as_str() == Some("pull_request_target")
                || event.as_sequence().is_some_and(|events| {
                    events
                        .iter()
                        .any(|event| event.as_str() == Some("pull_request_target"))
                })
                || event.as_mapping().is_some_and(|events| {
                    events.contains_key(Value::String("pull_request_target".into()))
                });
            if privileged {
                errors.push(format!(
                    "{location}: pull_request_target requires explicit opt-in"
                ));
            }
        }

        inspect(
            &document,
            &location,
            &sha,
            &digest,
            allow_inherit,
            &mut errors,
        );
    }

    for error in &errors {
        eprintln!("{error}");
    }

    ensure!(errors.is_empty(), "workflow security validation failed");
    println!("Validated {} workflow/action files", files.len());
    Ok(())
}

fn inspect(
    document: &Value,
    location: &str,
    sha: &Regex,
    digest: &Regex,
    allow_inherit: bool,
    errors: &mut Vec<String>,
) {
    match document {
        Value::Sequence(items) => {
            for (index, value) in items.iter().enumerate() {
                inspect(
                    value,
                    &format!("{location}[{index}]"),
                    sha,
                    digest,
                    allow_inherit,
                    errors,
                );
            }
        }

        Value::Mapping(items) => {
            if let Some(reference) = document["uses"].as_str() {
                if reference.starts_with("docker://") {
                    if !digest.is_match(reference) {
                        errors.push(format!(
                            "{location}: Docker action must use a SHA-256 digest"
                        ));
                    }
                } else if !reference.starts_with("./") && !sha.is_match(reference) {
                    errors.push(format!(
                        "{location}: action/workflow must use a full commit SHA: {reference}"
                    ));
                }

                if reference.starts_with("actions/checkout@")
                    && document["with"]["persist-credentials"] != Value::Bool(false)
                    && document["with"]["persist-credentials"].as_str() != Some("false")
                {
                    errors.push(format!(
                        "{location}: checkout must set persist-credentials: false"
                    ));
                }
            }

            if document["secrets"].as_str() == Some("inherit") && !allow_inherit {
                errors.push(format!(
                    "{location}: pass named secrets explicitly instead of inheriting all secrets"
                ));
            }

            for (key, value) in items {
                inspect(
                    value,
                    &format!("{location}.{}", key.as_str().unwrap_or("?")),
                    sha,
                    digest,
                    allow_inherit,
                    errors,
                );
            }
        }

        _ => {}
    }
}
