//! GitHub API operations used by composite actions.

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{command, github_output};

#[derive(Deserialize)]
struct Runners {
    runners: Vec<Runner>,
}

#[derive(Deserialize)]
struct Runner {
    status: String,
    busy: bool,
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

pub fn run(operation: &str) -> Result<()> {
    match operation {
        "select-runner" => select_runner(),
        "resolve-github-release" => resolve_release(),
        "terraform-plan-comment" => terraform_plan_comment(),
        "discover-repositories" => discover_repositories(),
        "configure-bot" => configure_bot(),
        _ => bail!("unknown GitHub operation: {operation}"),
    }
}

pub(crate) fn repository(value: &str) -> Result<()> {
    let valid_part = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    };
    let parts: Vec<_> = value.split('/').collect();
    ensure!(
        parts.len() == 2 && parts.iter().all(|part| valid_part(part)),
        "Repositories must be explicit owner/name pairs"
    );
    Ok(())
}

pub(crate) fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").expect("write to string");
        }
    }

    encoded
}

/// Keep gh pagination and request bodies explicit for every API side effect.
pub(crate) fn api(
    endpoint: &str,
    method: &str,
    body: Option<&Value>,
    paginate: bool,
) -> Result<Value> {
    let mut request = Command::new("gh");
    request.args(["api", endpoint, "--method", method]);
    if paginate {
        request.args(["--paginate", "--slurp"]);
    }

    let output = if let Some(body) = body {
        request
            .args(["--input", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = request.spawn().context("execute gh api")?;
        child
            .stdin
            .take()
            .context("open gh input")?
            .write_all(&serde_json::to_vec(body)?)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            return Err(command::Failure(output.status.code().unwrap_or(1)).into());
        }

        output.stdout
    } else {
        command::capture_bytes(&mut request)?
    };
    if output.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }

    let value: Value = serde_json::from_slice(&output).context("parse gh API response")?;
    if paginate {
        let pages = value
            .as_array()
            .context("expected paginated API response")?;
        let mut items = Vec::new();
        for page in pages {
            items.extend(
                page.as_array()
                    .context("expected API page array")?
                    .iter()
                    .cloned(),
            );
        }

        return Ok(Value::Array(items));
    }

    Ok(value)
}

fn select_runner() -> Result<()> {
    let preferred = env::var("PREFERRED_RUNNER")?;
    let fallback = env::var("FALLBACK_RUNNER")?;
    for label in [&preferred, &fallback] {
        ensure!(
            !label.trim().is_empty() && !label.contains(['\n', '\r']),
            "Runner labels must be nonempty single-line values"
        );
    }

    let base = env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".into());
    let repo = env::var("GITHUB_REPOSITORY")?;
    let token = env::var("RUNNER_STATUS_TOKEN").ok();
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("personal-ci")
        .build()?;
    let mut selected = &fallback;
    let mut self_hosted = false;
    for page in 1.. {
        let Some(token) = &token else {
            println!("::warning::Runner availability could not be confirmed; use hosted fallback.");
            break;
        };
        let url = format!(
            "{}/repos/{}/actions/runners?per_page=100&page={page}",
            base.trim_end_matches('/'),
            repo.split('/').map(encode).collect::<Vec<_>>().join("/")
        );
        let response = client
            .get(url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .send();
        let response = match response {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                println!(
                    "::warning::Runner lookup returned HTTP {}; use hosted fallback.",
                    response.status().as_u16()
                );
                break;
            }
            Err(_) => {
                println!(
                    "::warning::Runner availability could not be confirmed; use hosted fallback."
                );
                break;
            }
        };
        let runners: Runners = match response.json() {
            Ok(runners) => runners,
            Err(_) => {
                println!(
                    "::warning::Runner availability could not be confirmed; use hosted fallback."
                );
                break;
            }
        };
        if runners.runners.iter().any(|runner| {
            runner.status == "online"
                && !runner.busy
                && runner.labels.iter().any(|label| label.name == preferred)
        }) {
            selected = &preferred;
            self_hosted = true;
            break;
        }

        if runners.runners.len() < 100 {
            break;
        }
    }

    github_output::append("runner", selected)?;
    github_output::append("self-hosted", if self_hosted { "true" } else { "false" })?;
    println!("Selected runner: {selected}");
    Ok(())
}

fn resolve_release() -> Result<()> {
    let repo = env::var("REPO")?;
    repository(&repo)?;
    let version = env::var("VERSION")?;
    let tag = if version == "latest" {
        let base = env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".into());
        let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
        let url = format!(
            "{}/repos/{repo}/releases/latest",
            base.trim_end_matches('/')
        );
        let mut response = None;
        for attempt in 0..4 {
            let result = client.get(&url).header("User-Agent", "personal-ci").send();
            let retry = match &result {
                Ok(response) => {
                    response.status().as_u16() == 429
                        || matches!(response.status().as_u16(), 500 | 502 | 503 | 504)
                }
                Err(error) => error.is_timeout(),
            };
            if retry && attempt < 3 {
                std::thread::sleep(Duration::from_secs(1 << attempt));
                continue;
            }

            response = Some(result?);
            break;
        }

        let release: Value = response
            .context("missing release response")?
            .error_for_status()?
            .json()?;
        release["tag_name"]
            .as_str()
            .context("release tag must be a nonempty string")?
            .to_owned()
    } else {
        version
    };
    ensure!(
        !tag.is_empty() && !tag.contains(['\n', '\r']),
        "Release tag must be a nonempty single line"
    );
    github_output::append("tag", &tag)
}

fn terraform_plan_comment() -> Result<()> {
    let directory = env::var("WORKING_DIR")?;
    let environment = env::var("ENV_NAME")?;
    let repo = env::var("GITHUB_REPOSITORY")?;
    repository(&repo)?;
    let event: Value = serde_json::from_slice(&fs::read(env::var("GITHUB_EVENT_PATH")?)?)?;
    let number = event["issue"]["number"]
        .as_u64()
        .or_else(|| event["pull_request"]["number"].as_u64())
        .or_else(|| event["number"].as_u64())
        .context("event has no issue or pull request number")?;
    let plan = fs::read_to_string(Path::new(&directory).join("plan.txt"))?;
    // JavaScript's original bound counts UTF-16 code units, including surrogate pairs.
    let units: Vec<_> = plan.encode_utf16().collect();
    let output = if units.len() > 60000 {
        format!(
            "{}\n\n... (truncated)",
            String::from_utf16_lossy(&units[..60000])
        )
    } else {
        plan
    };
    let marker = format!(
        "<!-- terraform-plan:{} -->",
        hex::encode(Sha256::digest(format!("{directory}:{environment}")))
    );
    let body = format!(
        "{marker}\n#### Terraform Plan (`{environment}`)\n\n<details><summary>Plan output</summary>\n\n```hcl\n{output}\n```\n\n</details>"
    );
    let comments = api(
        &format!("repos/{repo}/issues/{number}/comments?per_page=100"),
        "GET",
        None,
        true,
    )?;
    let previous = comments
        .as_array()
        .context("expected comments")?
        .iter()
        .find(|comment| {
            comment["user"]["type"] == "Bot"
                && comment["body"]
                    .as_str()
                    .is_some_and(|body| body.starts_with(&marker))
        });
    let payload = serde_json::json!({"body": body});
    if let Some(comment) = previous {
        let id = comment["id"].as_u64().context("comment has no id")?;
        api(
            &format!("repos/{repo}/issues/comments/{id}"),
            "PATCH",
            Some(&payload),
            false,
        )?;
    } else {
        api(
            &format!("repos/{repo}/issues/{number}/comments"),
            "POST",
            Some(&payload),
            false,
        )?;
    }

    Ok(())
}

fn discover_repositories() -> Result<()> {
    let owner = env::var("OWNER")?;
    // Installation pagination returns objects instead of the arrays used elsewhere.
    let output = command::capture(Command::new("gh").args([
        "api",
        "/installation/repositories?per_page=100",
        "--paginate",
        "--slurp",
    ]))?;
    let pages: Vec<Value> = serde_json::from_str(&output)?;
    let mut names = Vec::new();
    for page in pages {
        for repo in page["repositories"]
            .as_array()
            .context("expected installation repositories")?
        {
            if repo["owner"]["login"] == owner && repo["archived"] == false && repo["fork"] == false
            {
                let name = repo["full_name"]
                    .as_str()
                    .context("repository missing full_name")?;
                repository(name)?;
                names.push(name.to_owned());
            }
        }
    }

    github_output::append("names", &names.join("\n"))
}

fn configure_bot() -> Result<()> {
    let login = format!("{}[bot]", env::var("SLUG")?);
    let user = api(&format!("users/{}", encode(&login)), "GET", None, false)?;
    let id = user["id"].as_u64().context("bot user has no numeric id")?;
    command::run(Command::new("git").args(["config", "--local", "user.name", &login]))?;
    command::run(Command::new("git").args([
        "config",
        "--local",
        "user.email",
        &format!("{id}+{login}@users.noreply.github.com"),
    ]))
}
