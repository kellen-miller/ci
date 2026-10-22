//! Warn before deleting unchanged stale branches in explicitly selected repositories.

use std::collections::{BTreeMap, HashSet};
use std::env;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::github::{api, encode, repository};

const MARKER: &str = "<!-- personal-ci-stale-branches:";

#[derive(Clone, Deserialize, Serialize)]
struct Warning {
    sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warned_at: Option<String>,
}

struct CleanupReport {
    number: u64,
    warnings: BTreeMap<String, Warning>,
}

impl CleanupReport {
    fn from_issues(issues: &Value, actor: &str, repo: &str) -> Result<Option<Self>> {
        let reports: Vec<_> = issues
            .as_array()
            .context("expected issues")?
            .iter()
            .filter(|issue| {
                issue["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(MARKER))
                    && issue.get("pull_request").is_none()
                    && issue["user"]["login"] == actor
            })
            .collect();
        ensure!(
            reports.len() <= 1,
            "Multiple cleanup reports in {repo}; resolve before proceeding"
        );
        let Some(report) = reports.first() else {
            return Ok(None);
        };

        let state = report["body"]
            .as_str()
            .context("report body missing")?
            .split_once(MARKER)
            .context("report marker missing")?
            .1
            .split_once(" -->")
            .context("report marker is unterminated")?
            .0;
        Ok(Some(Self {
            number: report["number"].as_u64().context("report number missing")?,
            warnings: serde_json::from_str(state).context("invalid cleanup warning state")?,
        }))
    }
}

/// Warn about stale branches or delete unchanged branches after their warning window.
///
/// # Errors
/// Returns an error for invalid configuration, API responses or persisted reports,
/// and for any failed GitHub API request.
pub fn run() -> Result<()> {
    let selected = env::var("REPOSITORIES")
        .ok()
        .filter(|value| !value.is_empty())
        .map(Ok)
        .unwrap_or_else(|| env::var("GITHUB_REPOSITORY"))?;
    let mut seen = HashSet::new();
    let repositories: Vec<_> = selected
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|repo| !repo.is_empty() && seen.insert(*repo))
        .collect();
    for repo in &repositories {
        repository(repo)?;
    }

    let stale_days: i64 = env::var("STALE_DAYS")
        .unwrap_or_else(|_| "30".into())
        .parse()?;
    let warning_days: i64 = env::var("WARNING_DAYS")
        .unwrap_or_else(|_| "7".into())
        .parse()?;
    let dry_run = env::var("DRY_RUN").unwrap_or_else(|_| "true".into());
    ensure!(
        stale_days >= 1 && warning_days >= 1 && matches!(dry_run.as_str(), "true" | "false"),
        "Days must be positive; DRY_RUN must be true or false"
    );
    let stale_window = Duration::try_days(stale_days).context("STALE_DAYS is too large")?;
    let warning_window = Duration::try_days(warning_days).context("WARNING_DAYS is too large")?;
    let now = Utc::now();
    let viewer = api(
        "graphql",
        "POST",
        Some(&serde_json::json!({"query": "query { viewer { login } }"})),
        false,
    )?;
    let actor = viewer["data"]["viewer"]["login"]
        .as_str()
        .context("viewer login missing")?;
    for repo in repositories {
        let metadata = api(&format!("repos/{repo}"), "GET", None, false)?;
        let archived = metadata["archived"]
            .as_bool()
            .context("repository archived flag missing")?;
        let fork = metadata["fork"]
            .as_bool()
            .context("repository fork flag missing")?;
        if archived || fork {
            println!("SKIP {repo} (archived or forked)");
            continue;
        }

        let default = metadata["default_branch"]
            .as_str()
            .context("default branch missing")?;
        let issues = api(
            &format!("repos/{repo}/issues?state=open&per_page=100"),
            "GET",
            None,
            true,
        )?;
        let report = CleanupReport::from_issues(&issues, actor, repo)?;
        let previous = report.as_ref().map(|report| &report.warnings);
        let mut warnings = BTreeMap::new();
        let mut deleted = Vec::new();
        let branches = api(
            &format!("repos/{repo}/branches?per_page=100"),
            "GET",
            None,
            true,
        )?;
        for branch in branches.as_array().context("expected branches")? {
            let name = branch["name"].as_str().context("branch name missing")?;
            let protected = branch["protected"]
                .as_bool()
                .context("branch protection missing")?;
            if name == default || protected {
                continue;
            }

            let encoded = encode(name);
            let head = encode(&format!(
                "{}:{name}",
                repo.split('/').next().context("repository owner missing")?
            ));
            let pulls_endpoint = format!("repos/{repo}/pulls?state=open&head={head}");
            let pulls = api(&pulls_endpoint, "GET", None, false)?;
            if !pulls
                .as_array()
                .context("expected pull requests")?
                .is_empty()
            {
                continue;
            }

            let sha = branch["commit"]["sha"]
                .as_str()
                .context("branch sha missing")?;
            let commit = api(
                &format!("repos/{repo}/commits/{}", encode(sha)),
                "GET",
                None,
                false,
            )?;
            let committed = DateTime::parse_from_rfc3339(
                commit["commit"]["committer"]["date"]
                    .as_str()
                    .context("commit date missing")?,
            )?;
            if now.signed_duration_since(committed) < stale_window {
                continue;
            }

            let warned = previous.and_then(|warnings| warnings.get(name));
            let mature = if let Some(warning) = warned.filter(|warning| warning.sha == sha) {
                match &warning.warned_at {
                    Some(date) => {
                        now.signed_duration_since(DateTime::parse_from_rfc3339(date)?)
                            >= warning_window
                    }
                    None => false,
                }
            } else {
                false
            };
            println!(
                "{} {repo} {name} (dry-run={dry_run})",
                if mature { "DELETE" } else { "WARN" }
            );
            if mature && dry_run == "false" {
                let current = api(
                    &format!("repos/{repo}/branches/{encoded}"),
                    "GET",
                    None,
                    false,
                )?;
                let protected = current["protected"]
                    .as_bool()
                    .context("current branch protection missing")?;
                if protected || current["commit"]["sha"] != sha {
                    continue;
                }

                let pulls = api(&pulls_endpoint, "GET", None, false)?;
                if !pulls
                    .as_array()
                    .context("expected current pull requests")?
                    .is_empty()
                {
                    continue;
                }

                api(
                    &format!("repos/{repo}/git/refs/heads/{encoded}"),
                    "DELETE",
                    None,
                    false,
                )?;
                deleted.push(name.to_owned());
            } else {
                let warning = warned
                    .filter(|warning| warning.sha == sha)
                    .cloned()
                    .unwrap_or_else(|| Warning {
                        sha: sha.to_owned(),
                        warned_at: Some(now.to_rfc3339()),
                    });
                warnings.insert(name.to_owned(), warning);
            }
        }

        if dry_run == "true" {
            continue;
        }

        let mut body = format!(
            "Stale unprotected branches without open PRs. Push a commit to keep a branch.\n\nBranches are eligible for deletion after {warning_days} days of warning.\n\n"
        );
        body.push_str(
            &warnings
                .keys()
                .map(|name| format!("- `{name}`"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        if !deleted.is_empty() {
            body.push_str("\n\nDeleted:\n");
            body.push_str(
                &deleted
                    .iter()
                    .map(|name| format!("- `{name}`"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }

        body.push_str(&format!(
            "\n\n{MARKER}{} -->",
            serde_json::to_string(&warnings)?
        ));
        if let Some(report) = report {
            api(
                &format!("repos/{repo}/issues/{}", report.number),
                "PATCH",
                Some(
                    &serde_json::json!({"body": body, "state": if warnings.is_empty() { "closed" } else { "open" }}),
                ),
                false,
            )?;
        } else if !warnings.is_empty() {
            api(
                &format!("repos/{repo}/issues"),
                "POST",
                Some(&serde_json::json!({"title": "Stale branch cleanup", "body": body})),
                false,
            )?;
        }
    }

    Ok(())
}
