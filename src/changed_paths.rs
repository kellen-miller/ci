//! Select CI groups using complete, NUL-delimited Git diffs.

use std::collections::BTreeMap;
use std::env;
use std::process::Command;

use anyhow::{Result, ensure};
use glob::{MatchOptions, Pattern};

use crate::{command, github_output};

pub type Filters = BTreeMap<String, Vec<String>>;

/// Match Python fnmatch-style globs, where a star also includes slashes.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn select(filters: &Filters, paths: &[String]) -> Result<BTreeMap<String, bool>> {
    ensure!(
        !filters.is_empty(),
        "filters must be a nonempty JSON object"
    );
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    let mut selected = BTreeMap::new();
    for (group, patterns) in filters {
        ensure!(
            !group.is_empty() && !patterns.is_empty(),
            "{group}: provide a nonempty list of glob patterns"
        );
        let mut matched = false;
        for pattern in patterns {
            ensure!(
                !pattern.is_empty() && !pattern.starts_with('!'),
                "{group}: only positive, nonempty glob patterns are supported"
            );
            // glob's recursive-star grammar differs from fnmatch: every run of
            // stars is one wildcard here. The root alternative is explicit.
            for alternative in std::iter::once(pattern.as_str()).chain(pattern.strip_prefix("**/"))
            {
                let normalized = regex::Regex::new(r"\*+")?.replace_all(alternative, "*");
                let matcher = Pattern::new(&normalized)?;
                matched |= paths.iter().any(|path| matcher.matches_with(path, options));
            }
        }

        selected.insert(group.clone(), matched);
    }

    Ok(selected)
}

/// Read the event contract and append selected groups to GITHUB_OUTPUT.
///
/// # Errors
/// Returns validation, file, network, or child-process failures to the caller.
pub fn run() -> Result<()> {
    let filters: Filters = serde_json::from_str(&env::var("FILTERS")?)?;
    // Validate even on non-PR events.
    let mut selected = select(&filters, &[])?;
    if env::var("EVENT_NAME")? == "pull_request" {
        let base = env::var("BASE_SHA")?;
        let head = env::var("HEAD_SHA")?;
        ensure!(
            !base.is_empty() && !head.is_empty(),
            "PR base and head SHAs are required"
        );
        let diff = command::capture_bytes(Command::new("git").args([
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &format!("{base}...{head}"),
            "--",
        ]))?;
        let paths = diff
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8(path.to_vec()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        selected = select(&filters, &paths)?;
    } else {
        selected.values_mut().for_each(|value| *value = true);
    }

    let changes = serde_json::to_string(&selected)?;
    github_output::append("changes", &changes)?;
    println!("CI groups: {changes}");
    Ok(())
}
