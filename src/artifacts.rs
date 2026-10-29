//! Retain one successful artifact set per pull request and workflow.

use std::collections::{BTreeMap, HashSet};
use std::env;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{github, github_output};

#[derive(Clone, Deserialize)]
struct Run {
    id: u64,
    workflow_id: u64,
    run_number: u64,
    event: String,
    status: String,
    conclusion: Option<String>,
    pull_requests: Vec<PullRequest>,
    created_at: String,
    head_sha: String,
    head_branch: Option<String>,
    head_repository: Option<HeadRepository>,
}

#[derive(Clone, Deserialize)]
struct HeadRepository {
    id: u64,
}

#[derive(Clone, Deserialize)]
struct PullRequest {
    number: u64,
}

#[derive(Clone, Deserialize)]
struct Artifact {
    id: u64,
    name: String,
    expired: bool,
    workflow_run: ArtifactRun,
}

#[derive(Clone, Deserialize)]
struct ArtifactRun {
    id: u64,
}

#[derive(Deserialize, Serialize)]
struct SourceEntry {
    path: String,
    mode: String,
    sha: String,
}

/// Select an unchanged build or remove superseded PR artifacts.
///
/// # Errors
/// Returns errors from invalid inputs, GitHub requests, or output writes.
pub fn run(mode: &str, dry_run: bool, include_failed: bool) -> Result<()> {
    ensure!(matches!(mode, "build" | "cleanup"), "invalid artifact mode");
    let repo = env::var("GITHUB_REPOSITORY")?;
    github::repository(&repo)?;
    let root = format!("repos/{repo}/actions");
    // Object-shaped pages are flattened here; github::api's paginator expects arrays.
    let mut artifacts = Vec::new();
    let mut page = 1;
    loop {
        let response = github::api(
            &format!("{root}/artifacts?per_page=100&page={page}"),
            "GET",
            None,
            false,
        )?;
        let batch: Vec<Artifact> = serde_json::from_value(response["artifacts"].clone())?;
        let complete = batch.len() < 100;
        artifacts.extend(batch.into_iter().filter(|artifact| !artifact.expired));
        if complete {
            break;
        }

        page += 1;
    }

    let mut runs = BTreeMap::new();
    for artifact in &artifacts {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            runs.entry(artifact.workflow_run.id)
        {
            let Some(response) =
                github::api_if_exists(&format!("{root}/runs/{}", artifact.workflow_run.id), "GET")?
            else {
                continue;
            };
            let mut run: Run = serde_json::from_value(response)?;
            if run.event == "pull_request" && run.pull_requests.is_empty() {
                let pulls = github::api(
                    &format!("repos/{repo}/commits/{}/pulls?per_page=100", run.head_sha),
                    "GET",
                    None,
                    true,
                )?;
                run.pull_requests = pulls
                    .as_array()
                    .context("missing associated PRs")?
                    .iter()
                    .filter(|pr| associated_pr(&run, pr, &repo))
                    .map(|pr| serde_json::from_value(pr.clone()))
                    .collect::<std::result::Result<_, _>>()?;
            }

            entry.insert(run);
        }
    }

    if mode == "build" {
        let event: Value = serde_json::from_slice(&std::fs::read(env::var("GITHUB_EVENT_PATH")?)?)?;
        let number = event["pull_request"]["number"].as_u64();
        let current_id: u64 = env::var("GITHUB_RUN_ID")?.parse()?;
        let current: Run = serde_json::from_value(github::api(
            &format!("{root}/runs/{current_id}"),
            "GET",
            None,
            false,
        )?)?;
        let sha = env::var("GITHUB_SHA")?;
        ensure!(
            sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid commit"
        );
        let commit = github::api(
            &format!("repos/{repo}/git/commits/{sha}"),
            "GET",
            None,
            false,
        )?;
        let tree = commit["tree"]["sha"]
            .as_str()
            .context("missing source tree")?;
        let tree = github::api(
            &format!("repos/{repo}/git/trees/{tree}?recursive=1"),
            "GET",
            None,
            false,
        )?;
        let paths: Vec<String> = serde_json::from_str(&env::var("SOURCE_PATHS")?)?;
        let source = source_marker(&tree, &paths)?;
        let mut required: Vec<String> = serde_json::from_str(&env::var("REQUIRED_ARTIFACTS")?)?;
        ensure!(!required.is_empty(), "declare the complete artifact set");
        let reusable = number.and_then(|number| {
            runs.values()
                .filter(|run| {
                    run.id != current_id
                        && run.workflow_id == current.workflow_id
                        && run.event == "pull_request"
                        && run.conclusion.as_deref() == Some("success")
                        && run.pull_requests.iter().any(|pr| pr.number == number)
                        && artifacts.iter().any(|artifact| {
                            artifact.workflow_run.id == run.id && artifact.name == source
                        })
                        && required.iter().all(|name| {
                            artifacts.iter().any(|artifact| {
                                artifact.workflow_run.id == run.id && artifact.name == *name
                            })
                        })
                })
                .max_by_key(|run| run.run_number)
        });
        // Verify the producer's inputs too; a marker name alone is not provenance.
        let reusable = if let Some(run) = reusable {
            let commit = github::api(
                &format!("repos/{repo}/git/commits/{}", run.head_sha),
                "GET",
                None,
                false,
            )?;
            let tree = commit["tree"]["sha"]
                .as_str()
                .context("missing producer source tree")?;
            let tree = github::api(
                &format!("repos/{repo}/git/trees/{tree}?recursive=1"),
                "GET",
                None,
                false,
            )?;
            (source_marker(&tree, &paths)? == source).then_some(run)
        } else {
            None
        };
        github_output::append("run-id", &reusable.map_or(0, |run| run.id).to_string())?;
        required.push(source.clone());
        github_output::append("artifacts", &serde_json::to_string(&required)?)?;
        github_output::append("build", if reusable.is_some() { "false" } else { "true" })?;
        github_output::append("source", &source)?;
        println!("Build required: {} ({source})", reusable.is_none());
        return Ok(());
    }

    let retained = retained_runs(&runs, &artifacts, include_failed);
    let transient_prefix = env::var("TRANSIENT_PREFIX").unwrap_or_default();
    for artifact in &artifacts {
        let Some(run) = runs.get(&artifact.workflow_run.id) else {
            continue;
        };
        if run.event != "pull_request" || run.status != "completed" || run.pull_requests.len() != 1
        {
            continue;
        }

        let key = (run.workflow_id, run.pull_requests[0].number);
        let Some(latest) = retained.get(&key).and_then(|id| runs.get(id)) else {
            continue;
        };
        // A partial rerun can legitimately reuse earlier attempts' final outputs.
        let consumed_input = !transient_prefix.is_empty()
            && artifact.name.starts_with(&transient_prefix)
            && run.conclusion.as_deref() == Some("success")
            && artifacts.iter().any(|output| {
                output.workflow_run.id == run.id
                    && !output.name.starts_with(&transient_prefix)
                    && !output.name.starts_with("pr-source-")
            });
        let superseded = run.run_number < latest.run_number || consumed_input;
        if !superseded {
            continue;
        }

        println!(
            "{} {repo} artifact {} ({}) for PR #{}",
            if dry_run { "WOULD DELETE" } else { "DELETE" },
            artifact.id,
            artifact.name,
            key.1
        );
        if !dry_run {
            github::api_if_exists(&format!("{root}/artifacts/{}", artifact.id), "DELETE")?;
        }
    }

    Ok(())
}

fn source_marker(tree: &Value, paths: &[String]) -> Result<String> {
    ensure!(tree["truncated"] == false, "source tree must be complete");
    ensure!(!paths.is_empty(), "declare the build inputs");
    let patterns = paths
        .iter()
        .map(|path| glob::Pattern::new(path))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut entries = Vec::new();
    for entry in tree["tree"].as_array().context("missing source entries")? {
        if entry["type"] == "tree" {
            continue;
        }

        let entry: SourceEntry = serde_json::from_value(entry.clone())?;
        if patterns.iter().any(|pattern| pattern.matches(&entry.path)) {
            entries.push(entry);
        }
    }

    ensure!(!entries.is_empty(), "no build inputs matched");
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let mut digest = Sha256::new();
    digest.update(b"pr-artifact-source-v1\0");
    digest.update(serde_json::to_vec(&(paths, entries))?);
    Ok(format!("pr-source-{}", hex::encode(digest.finalize())))
}

fn retained_runs(
    runs: &BTreeMap<u64, Run>,
    artifacts: &[Artifact],
    include_failed: bool,
) -> BTreeMap<(u64, u64), u64> {
    let available: HashSet<_> = artifacts
        .iter()
        .filter(|artifact| !artifact.name.starts_with("pr-source-"))
        .map(|artifact| artifact.workflow_run.id)
        .collect();
    let mut latest = BTreeMap::new();
    for run in runs.values() {
        if run.event != "pull_request"
            || run.status != "completed"
            || !(run.conclusion.as_deref() == Some("success")
                || (include_failed && run.conclusion.as_deref() == Some("failure")))
            || run.pull_requests.len() != 1
            || !available.contains(&run.id)
        {
            continue;
        }

        let key = (run.workflow_id, run.pull_requests[0].number);
        let previous = latest.entry(key).or_insert(run.id);
        if runs[previous].run_number < run.run_number {
            *previous = run.id;
        }
    }

    latest
}

// GitHub often omits pull_requests on completed runs after their branch is deleted.
fn associated_pr(run: &Run, pr: &Value, repo: &str) -> bool {
    pr["base"]["repo"]["full_name"].as_str() == Some(repo)
        && pr["head"]["ref"].as_str() == run.head_branch.as_deref()
        && run
            .head_repository
            .as_ref()
            .is_some_and(|head| pr["head"]["repo"]["id"].as_u64() == Some(head.id))
        && pr["created_at"]
            .as_str()
            .is_some_and(|created| created <= run.created_at.as_str())
        && pr["closed_at"]
            .as_str()
            .is_none_or(|closed| run.created_at.as_str() <= closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_identity_ignores_docs_but_tracks_code_modes_and_deletions() {
        let mut tree = json!({"truncated": false, "tree": [
            {"path":"src/main.rs","mode":"100644","sha":"code","type":"blob"},
            {"path":"README.md","mode":"100644","sha":"docs","type":"blob"}
        ]});
        let paths = vec!["src/**".into(), "Cargo.toml".into()];
        let original = source_marker(&tree, &paths).unwrap();
        tree["tree"][1]["sha"] = json!("new-docs");
        assert_eq!(source_marker(&tree, &paths).unwrap(), original);
        tree["tree"][0]["mode"] = json!("100755");
        assert_ne!(source_marker(&tree, &paths).unwrap(), original);
        tree["tree"][0]["mode"] = json!("100644");
        tree["tree"][0]["sha"] = json!("new-code");
        assert_ne!(source_marker(&tree, &paths).unwrap(), original);
        tree["tree"][0]["path"] = json!("Cargo.toml");
        assert_ne!(source_marker(&tree, &paths).unwrap(), original);
        tree["truncated"] = json!(true);
        assert!(source_marker(&tree, &paths).is_err());
    }

    fn run_fixture(id: u64, workflow: u64, pr: u64, conclusion: &str) -> Run {
        serde_json::from_value(json!({
            "id": id, "workflow_id": workflow, "run_number": id,
            "event": "pull_request", "status": "completed", "conclusion": conclusion,
            "pull_requests": [{"number": pr}], "run_started_at": "2026-10-29T12:00:00Z",
            "created_at": "2026-10-29T12:00:00Z", "head_sha": "abc",
            "head_branch": "feature", "head_repository": {"id": 42}
        }))
        .unwrap()
    }

    fn artifact_fixture(run: u64, name: &str) -> Artifact {
        serde_json::from_value(json!({
            "id": run, "name": name, "expired": false,
            "created_at": "2026-10-29T12:01:00Z", "workflow_run": {"id": run}
        }))
        .unwrap()
    }

    #[test]
    fn retain_latest_success_per_pr_and_workflow() {
        let runs = [
            run_fixture(1, 10, 1, "success"),
            run_fixture(2, 10, 1, "success"),
            run_fixture(3, 11, 1, "success"),
            run_fixture(4, 10, 2, "success"),
            run_fixture(5, 10, 1, "failure"),
        ];
        let artifacts: Vec<_> = runs
            .iter()
            .map(|run| artifact_fixture(run.id, "package"))
            .collect();
        let runs = runs.into_iter().map(|run| (run.id, run)).collect();
        assert_eq!(
            retained_runs(&runs, &artifacts, false),
            BTreeMap::from([((10, 1), 2), ((11, 1), 3), ((10, 2), 4)])
        );
    }

    #[test]
    fn skipped_build_and_source_marker_cannot_replace_a_package() {
        let runs = [
            run_fixture(1, 10, 1, "success"),
            run_fixture(2, 10, 1, "success"),
            run_fixture(3, 10, 1, "success"),
        ]
        .into_iter()
        .map(|run| (run.id, run))
        .collect();
        let artifacts = [
            artifact_fixture(1, "package"),
            artifact_fixture(3, "pr-source-abc"),
        ];
        assert_eq!(
            retained_runs(&runs, &artifacts, false),
            BTreeMap::from([((10, 1), 1)])
        );
    }

    #[test]
    fn ambiguous_pr_association_and_push_runs_are_preserved() {
        let mut run = run_fixture(1, 10, 1, "success");
        run.pull_requests.push(PullRequest { number: 2 });
        let mut push = run_fixture(2, 10, 1, "success");
        push.event = "push".into();
        let runs = [(1, run), (2, push)].into_iter().collect();
        assert!(
            retained_runs(
                &runs,
                &[
                    artifact_fixture(1, "package"),
                    artifact_fixture(2, "package")
                ],
                false,
            )
            .is_empty()
        );
    }

    #[test]
    fn diagnostic_reports_retain_only_the_latest_completed_failure() {
        let runs = [
            run_fixture(1, 10, 1, "failure"),
            run_fixture(2, 10, 1, "failure"),
        ]
        .into_iter()
        .map(|run| (run.id, run))
        .collect();
        let artifacts = [artifact_fixture(1, "report"), artifact_fixture(2, "report")];
        assert_eq!(
            retained_runs(&runs, &artifacts, true),
            BTreeMap::from([((10, 1), 2)])
        );
        assert!(retained_runs(&runs, &artifacts, false).is_empty());
    }

    #[test]
    fn resolve_deleted_branches_without_matching_reused_or_fork_branches() {
        let run = run_fixture(1, 10, 1, "success");
        let mut pr = json!({"created_at":"2026-10-28T12:00:00Z", "closed_at":null,
            "head":{"ref":"feature","repo":{"id":42}}, "base":{"repo":{"full_name":"owner/repo"}}});
        assert!(associated_pr(&run, &pr, "owner/repo"));
        pr["head"]["repo"]["id"] = json!(99);
        assert!(!associated_pr(&run, &pr, "owner/repo"));
        pr["head"]["repo"]["id"] = json!(42);
        pr["closed_at"] = json!("2026-10-28T13:00:00Z");
        assert!(!associated_pr(&run, &pr, "owner/repo"));
        pr["closed_at"] = Value::Null;
        pr["created_at"] = json!("2026-10-30T13:00:00Z");
        assert!(!associated_pr(&run, &pr, "owner/repo"));
    }
}
