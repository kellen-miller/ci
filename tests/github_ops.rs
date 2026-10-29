#![cfg(test)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::thread;

use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

struct Sandbox {
    directory: TempDir,
}

struct Call {
    method: String,
    endpoint: String,
    arguments: String,
    body: Value,
}

struct HttpFixture {
    base: String,
    handle: thread::JoinHandle<Vec<String>>,
}

impl HttpFixture {
    fn new(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    assert_eq!(stream.read(&mut byte).unwrap(), 1);
                    request.push(byte[0]);
                }

                requests.push(String::from_utf8(request).unwrap());
                write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }

            requests
        });
        Self { base, handle }
    }
}

impl Sandbox {
    fn new() -> Self {
        static GH: OnceLock<TempDir> = OnceLock::new();
        let fixture = GH.get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            let result = Command::new("rustc")
                .args(["--edition", "2024"])
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github_api.rs"))
                .arg("-o")
                .arg(directory.path().join("gh"))
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            directory
        });
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("calls")).unwrap();
        fs::create_dir(directory.path().join("bin")).unwrap();
        fs::copy(fixture.path().join("gh"), directory.path().join("bin/gh")).unwrap();
        Self { directory }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn response(&self, method: &str, endpoint: &str, index: usize, response: Value) {
        let key: String = endpoint.bytes().map(|byte| format!("{byte:02x}")).collect();
        fs::write(
            self.root().join(format!("{method}-{key}.{index}")),
            serde_json::to_vec(&response).unwrap(),
        )
        .unwrap();
    }

    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", "operation_dispatch", "--nocapture"])
            .current_dir(self.root())
            .env("CI_TEST_OPERATION", operation)
            .env("GH_FIXTURE_DIR", self.root())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root().join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("GITHUB_OUTPUT", self.root().join("output"))
            .env("GITHUB_REPOSITORY", "example/project")
            .env("REPOSITORIES", "example/project")
            .env("STALE_DAYS", "30")
            .env("WARNING_DAYS", "7")
            .env("DRY_RUN", "true")
            .env("GH_TOKEN", "fixture-secret");
        command
    }

    fn calls(&self) -> Vec<Call> {
        let mut paths: Vec<PathBuf> = fs::read_dir(self.root().join("calls"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let text = fs::read_to_string(path).unwrap();
                let mut lines = text.splitn(4, '\n');
                Call {
                    method: lines.next().unwrap().into(),
                    endpoint: lines.next().unwrap().into(),
                    arguments: lines.next().unwrap().into(),
                    body: serde_json::from_str(lines.next().unwrap()).unwrap_or(Value::Null),
                }
            })
            .collect()
    }

    fn branch_fixture(&self, issues: Value) {
        self.response(
            "POST",
            "graphql",
            0,
            json!({"data":{"viewer":{"login":"ci[bot]"}}}),
        );
        self.response(
            "GET",
            "repos/example/project",
            0,
            json!({"archived":false,"fork":false,"default_branch":"main"}),
        );
        self.response(
            "GET",
            "repos/example/project/issues?state=open&per_page=100",
            0,
            json!([issues]),
        );
        self.response("GET", "repos/example/project/branches?per_page=100", 0, json!([[{"name":"main","protected":false,"commit":{"sha":"old"}},{"name":"protected","protected":true,"commit":{"sha":"old"}}],[{"name":"has-pr","protected":false,"commit":{"sha":"old"}},{"name":"stale/topic","protected":false,"commit":{"sha":"old"}},{"name":"fresh","protected":false,"commit":{"sha":"new"}}]]));
        self.response(
            "GET",
            "repos/example/project/pulls?state=open&head=example%3Ahas-pr",
            0,
            json!([{}]),
        );
        self.response(
            "GET",
            "repos/example/project/pulls?state=open&head=example%3Astale%2Ftopic",
            0,
            json!([]),
        );
        self.response(
            "GET",
            "repos/example/project/pulls?state=open&head=example%3Afresh",
            0,
            json!([]),
        );
        self.response(
            "GET",
            "repos/example/project/commits/old",
            0,
            json!({"commit":{"committer":{"date":(Utc::now()-Duration::days(60)).to_rfc3339()}}}),
        );
        self.response(
            "GET",
            "repos/example/project/commits/new",
            0,
            json!({"commit":{"committer":{"date":Utc::now().to_rfc3339()}}}),
        );
        self.response("POST", "repos/example/project/issues", 0, json!({}));
        self.response("PATCH", "repos/example/project/issues/1", 0, json!({}));
        self.response(
            "GET",
            "repos/example/project/branches/stale%2Ftopic",
            0,
            json!({"protected":false,"commit":{"sha":"old"}}),
        );
        self.response(
            "DELETE",
            "repos/example/project/git/refs/heads/stale%2Ftopic",
            0,
            Value::Null,
        );
    }
}

fn successful(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn report(login: &str, days: i64, sha: &str) -> Value {
    let state = json!({"stale/topic":{"sha":sha,"warned_at":(Utc::now()-Duration::days(days)).to_rfc3339()}});
    json!({"number":1,"user":{"login":login},"body":format!("<!-- personal-ci-stale-branches:{state} -->")})
}

#[test]
#[ignore = "isolated subprocess entry point"]
fn operation_dispatch() {
    let operation = std::env::var("CI_TEST_OPERATION").unwrap();
    let result = if operation == "stale-branch-cleanup" {
        personal_ci::branches::run()
    } else if operation == "pr-artifact-build" {
        personal_ci::artifacts::run("build", false, false)
    } else if operation == "pr-artifact-cleanup" {
        personal_ci::artifacts::run("cleanup", false, false)
    } else {
        personal_ci::github::run(&operation)
    };

    if let Err(error) = result {
        panic!("{error:#}");
    }
}

#[test]
fn runner_selection_paginates_and_authenticates() {
    let sandbox = Sandbox::new();
    let busy = json!({"status":"online","busy":true,"labels":[{"name":"home"}]});
    let server = HttpFixture::new(vec![
        (200, json!({"runners":vec![busy;100]}).to_string()),
        (
            200,
            json!({"runners":[{"status":"online","busy":false,"labels":[{"name":"home"}]}]})
                .to_string(),
        ),
    ]);
    successful(
        sandbox
            .command("select-runner")
            .env("GITHUB_API_URL", &server.base)
            .env("PREFERRED_RUNNER", "home")
            .env("FALLBACK_RUNNER", "ubuntu-24.04")
            .env("RUNNER_STATUS_TOKEN", "fixture-secret")
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(sandbox.root().join("output")).unwrap(),
        "runner=home\nself-hosted=true\n"
    );
    let requests = server.handle.join().unwrap();
    assert!(requests[0].contains("per_page=100&page=1"));
    assert!(requests[1].contains("per_page=100&page=2"));
    assert!(requests[0].contains("authorization: Bearer fixture-secret"));
    assert!(requests[0].contains("x-github-api-version: 2026-03-10"));
}

#[test]
fn runner_falls_back_on_unavailable_or_invalid_capacity_without_token() {
    for (status, body) in [(403,"{}".into()), (200,"invalid".into()),(200,json!({"runners":[{"status":"offline","busy":false,"labels":[{"name":"home"}]},{"status":"online","busy":true,"labels":[{"name":"home"}]},{"status":"online","busy":false,"labels":[{"name":"other"}]}]}).to_string()), (200,json!({"runners":[{"busy":false}]}).to_string())] {
        let sandbox = Sandbox::new();
        let server = HttpFixture::new(vec![(status,body)]);
        let output = sandbox.command("select-runner").env("GITHUB_API_URL", &server.base).env("PREFERRED_RUNNER","home").env("FALLBACK_RUNNER","ubuntu-24.04").env("RUNNER_STATUS_TOKEN","fixture-secret").output().unwrap();
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        let stdout = successful(output);
        assert!(!stdout.contains("fixture-secret"));
        if status == 403 { assert!(stdout.contains("HTTP 403")); }

        assert_eq!(fs::read_to_string(sandbox.root().join("output")).unwrap(),"runner=ubuntu-24.04\nself-hosted=false\n");
        server.handle.join().unwrap();
    }
}

#[test]
fn runner_rejects_multiline_or_empty_labels_before_network() {
    for label in ["", "  ", "home\nevil", "home\revil"] {
        let sandbox = Sandbox::new();
        let output = sandbox
            .command("select-runner")
            .env("PREFERRED_RUNNER", label)
            .env("FALLBACK_RUNNER", "ubuntu-latest")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!sandbox.root().join("output").exists());
    }
}

#[test]
fn release_resolves_latest_and_preserves_explicit_tag() {
    let sandbox = Sandbox::new();
    let server = HttpFixture::new(vec![(200, json!({"tag_name":"v1.2.3"}).to_string())]);
    successful(
        sandbox
            .command("resolve-github-release")
            .env("REPO", "example/project")
            .env("VERSION", "latest")
            .env("GITHUB_API_URL", &server.base)
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(sandbox.root().join("output")).unwrap(),
        "tag=v1.2.3\n"
    );
    assert!(server.handle.join().unwrap()[0].contains("/repos/example/project/releases/latest"));
    successful(
        sandbox
            .command("resolve-github-release")
            .env("REPO", "example/project")
            .env("VERSION", "release candidate")
            .output()
            .unwrap(),
    );
    assert!(
        fs::read_to_string(sandbox.root().join("output"))
            .unwrap()
            .ends_with("tag=release candidate\n")
    );
}

#[test]
fn release_rejects_invalid_repository_and_tag() {
    for (repo, version) in [
        ("../example/project", "v1"),
        ("example/project", ""),
        ("example/project", "v1\nbad"),
    ] {
        let sandbox = Sandbox::new();
        assert!(
            !sandbox
                .command("resolve-github-release")
                .env("REPO", repo)
                .env("VERSION", version)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    for body in [json!({}), json!({"tag_name":3}), json!({"tag_name":""})] {
        let sandbox = Sandbox::new();
        let server = HttpFixture::new(vec![(200, body.to_string())]);
        assert!(
            !sandbox
                .command("resolve-github-release")
                .env("REPO", "example/project")
                .env("VERSION", "latest")
                .env("GITHUB_API_URL", &server.base)
                .output()
                .unwrap()
                .status
                .success()
        );
        server.handle.join().unwrap();
    }
}

#[test]
fn terraform_comment_updates_only_matching_bot_and_truncates() {
    for update in [true, false] {
        let sandbox = Sandbox::new();
        fs::write(sandbox.root().join("plan.txt"), "a".repeat(60001)).unwrap();
        fs::write(
            sandbox.root().join("event.json"),
            json!({"pull_request":{"number":42}}).to_string(),
        )
        .unwrap();
        let marker = format!(
            "<!-- terraform-plan:{} -->",
            hex::encode(Sha256::digest(".:production"))
        );
        let mut comments = vec![
            json!({"id":1,"user":{"type":"User"},"body":marker}),
            json!({"id":2,"user":{"type":"Bot"},"body":"<!-- terraform-plan:unrelated -->"}),
        ];
        if update {
            comments.push(json!({"id":99,"user":{"type":"Bot"},"body":format!("{marker}\nold")}));
        }

        sandbox.response(
            "GET",
            "repos/example/project/issues/42/comments?per_page=100",
            0,
            json!([[comments[0]], comments[1..].to_vec()]),
        );
        sandbox.response(
            if update { "PATCH" } else { "POST" },
            if update {
                "repos/example/project/issues/comments/99"
            } else {
                "repos/example/project/issues/42/comments"
            },
            0,
            json!({}),
        );
        successful(
            sandbox
                .command("terraform-plan-comment")
                .env("WORKING_DIR", ".")
                .env("ENV_NAME", "production")
                .env("GITHUB_EVENT_PATH", sandbox.root().join("event.json"))
                .output()
                .unwrap(),
        );
        let calls = sandbox.calls();
        assert!(calls[0].arguments.contains("--paginate\t--slurp"));
        let body = calls[1].body["body"].as_str().unwrap();
        assert!(body.starts_with(&marker));
        assert!(body.contains(&format!("{}\n\n... (truncated)", "a".repeat(60000))));
        assert!(!body.contains(&"a".repeat(60001)));
        assert!(body.ends_with("```\n\n</details>"));
    }
}

#[test]
fn discovery_filters_installation_pages_and_configures_local_bot() {
    let sandbox = Sandbox::new();
    sandbox.response("GET","/installation/repositories?per_page=100",0,json!([{"repositories":[{"owner":{"login":"example"},"archived":false,"fork":false,"full_name":"example/one"},{"owner":{"login":"other"},"archived":false,"fork":false,"full_name":"other/two"}]},{"repositories":[{"owner":{"login":"example"},"archived":true,"fork":false,"full_name":"example/archive"},{"owner":{"login":"example"},"archived":false,"fork":true,"full_name":"example/fork"},{"owner":{"login":"example"},"archived":false,"fork":false,"full_name":"example/three"}]}]));
    successful(
        sandbox
            .command("discover-repositories")
            .env("OWNER", "example")
            .output()
            .unwrap(),
    );
    let output = fs::read_to_string(sandbox.root().join("output")).unwrap();
    assert!(output.contains("example/one\nexample/three"));
    assert!(!output.contains("archive"));
    assert!(!output.contains("other/two"));
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(sandbox.root())
            .status()
            .unwrap()
            .success()
    );
    sandbox.response("GET", "users/my-app%5Bbot%5D", 0, json!({"id":123}));
    successful(
        sandbox
            .command("configure-bot")
            .env("SLUG", "my-app")
            .output()
            .unwrap(),
    );
    for (key, value) in [
        ("user.name", "my-app[bot]"),
        ("user.email", "123+my-app[bot]@users.noreply.github.com"),
    ] {
        let output = Command::new("git")
            .args(["config", "--local", "--get", key])
            .current_dir(sandbox.root())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), value);
    }
}

#[test]
fn branch_dry_run_has_no_writes_and_warns_only_stale_candidates() {
    let sandbox = Sandbox::new();
    sandbox.branch_fixture(json!([]));
    let output = successful(
        sandbox
            .command("stale-branch-cleanup")
            .env(
                "REPOSITORIES",
                "example/project, example/project\nexample/project",
            )
            .output()
            .unwrap(),
    );
    assert!(output.contains("WARN example/project stale/topic (dry-run=true)"));
    assert!(!output.contains("WARN example/project fresh"));
    let calls = sandbox.calls();
    assert!(
        calls
            .iter()
            .all(|call| call.method == "GET" || call.endpoint == "graphql")
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.endpoint == "repos/example/project")
            .count(),
        1
    );
    assert!(
        calls
            .iter()
            .filter(|call| call.endpoint.contains("branches?") || call.endpoint.contains("issues?"))
            .all(|call| call.arguments.contains("--paginate\t--slurp"))
    );
}

#[test]
fn branch_warning_is_owned_by_actor_and_preserves_warning_window() {
    for issues in [
        json!([]),
        json!([report("other[bot]", 60, "old")]),
        json!([report("ci[bot]", 1, "old")]),
        json!([report("ci[bot]", 60, "changed")]),
    ] {
        let sandbox = Sandbox::new();
        sandbox.branch_fixture(issues);
        successful(
            sandbox
                .command("stale-branch-cleanup")
                .env("DRY_RUN", "false")
                .output()
                .unwrap(),
        );
        let calls = sandbox.calls();
        assert!(!calls.iter().any(|call| call.method == "DELETE"));
        let write = calls.last().unwrap();
        assert!(matches!(write.method.as_str(), "POST" | "PATCH"));
        let body = write.body["body"].as_str().unwrap();
        assert!(body.contains("stale/topic"));
        assert!(!body.contains("has-pr"));
        assert!(!body.contains("`protected`"));
    }
}

#[test]
fn branch_deletion_rechecks_tip_protection_and_open_prs() {
    for condition in ["unchanged", "changed", "protected", "new-pr"] {
        let sandbox = Sandbox::new();
        sandbox.branch_fixture(json!([report("ci[bot]", 60, "old")]));
        if condition == "changed" {
            sandbox.response(
                "GET",
                "repos/example/project/branches/stale%2Ftopic",
                0,
                json!({"protected":false,"commit":{"sha":"new"}}),
            );
        }

        if condition == "protected" {
            sandbox.response(
                "GET",
                "repos/example/project/branches/stale%2Ftopic",
                0,
                json!({"protected":true,"commit":{"sha":"old"}}),
            );
        }

        if condition == "new-pr" {
            sandbox.response(
                "GET",
                "repos/example/project/pulls?state=open&head=example%3Astale%2Ftopic",
                1,
                json!([{}]),
            );
        }

        successful(
            sandbox
                .command("stale-branch-cleanup")
                .env("DRY_RUN", "false")
                .output()
                .unwrap(),
        );
        let calls = sandbox.calls();
        assert_eq!(
            calls.iter().filter(|call| call.method == "DELETE").count(),
            usize::from(condition == "unchanged")
        );
        assert_eq!(calls.last().unwrap().body["state"], "closed");
        if condition == "unchanged" {
            let deletion = calls
                .iter()
                .position(|call| call.method == "DELETE")
                .unwrap();
            assert!(calls[deletion - 1].endpoint.contains("/pulls?"));
            assert!(
                calls[deletion - 2]
                    .endpoint
                    .contains("/branches/stale%2Ftopic")
            );
        }
    }
}

#[test]
fn branches_skip_archives_and_forks_before_branch_or_issue_access() {
    for (archived, fork) in [(true, false), (false, true), (true, true)] {
        let sandbox = Sandbox::new();
        sandbox.branch_fixture(json!([]));
        sandbox.response(
            "GET",
            "repos/example/project",
            0,
            json!({"archived":archived,"fork":fork}),
        );
        assert!(
            successful(
                sandbox
                    .command("stale-branch-cleanup")
                    .env("DRY_RUN", "false")
                    .output()
                    .unwrap()
            )
            .contains("SKIP example/project")
        );
        assert_eq!(sandbox.calls().len(), 2);
    }
}

#[test]
fn branches_fail_closed_for_duplicate_reports_or_bad_state() {
    for issues in [
        json!([report("ci[bot]", 60, "old"), report("ci[bot]", 60, "old")]),
        json!([{"number":1,"user":{"login":"ci[bot]"},"body":"<!-- personal-ci-stale-branches:invalid -->"}]),
    ] {
        let sandbox = Sandbox::new();
        sandbox.branch_fixture(issues);
        assert!(
            !sandbox
                .command("stale-branch-cleanup")
                .env("DRY_RUN", "false")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert_eq!(sandbox.calls().len(), 3);
    }
}

#[test]
fn branches_reject_unsafe_configuration_before_api_access() {
    for (key, value) in [
        ("REPOSITORIES", "example"),
        ("REPOSITORIES", "example/project/extra"),
        ("STALE_DAYS", "0"),
        ("WARNING_DAYS", "-1"),
        ("DRY_RUN", "yes"),
    ] {
        let sandbox = Sandbox::new();
        assert!(
            !sandbox
                .command("stale-branch-cleanup")
                .env(key, value)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(sandbox.calls().is_empty());
    }
}

#[test]
fn runner_missing_token_uses_fallback_without_requesting_api() {
    let sandbox = Sandbox::new();
    successful(
        sandbox
            .command("select-runner")
            .env("GITHUB_API_URL", "http://127.0.0.1:1")
            .env("PREFERRED_RUNNER", "home")
            .env("FALLBACK_RUNNER", "ubuntu-latest")
            .env_remove("RUNNER_STATUS_TOKEN")
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(sandbox.root().join("output")).unwrap(),
        "runner=ubuntu-latest\nself-hosted=false\n"
    );
}

#[test]
fn release_retries_transient_http_failure() {
    let sandbox = Sandbox::new();
    let server = HttpFixture::new(vec![
        (503, "{}".into()),
        (200, json!({"tag_name":"v2"}).to_string()),
    ]);
    successful(
        sandbox
            .command("resolve-github-release")
            .env("REPO", "example/project")
            .env("VERSION", "latest")
            .env("GITHUB_API_URL", &server.base)
            .output()
            .unwrap(),
    );
    assert_eq!(server.handle.join().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(sandbox.root().join("output")).unwrap(),
        "tag=v2\n"
    );
}

#[test]
fn branch_dry_run_never_deletes_mature_warnings() {
    let sandbox = Sandbox::new();
    sandbox.branch_fixture(json!([report("ci[bot]", 60, "old")]));
    assert!(
        successful(sandbox.command("stale-branch-cleanup").output().unwrap())
            .contains("DELETE example/project stale/topic (dry-run=true)")
    );
    assert!(
        sandbox
            .calls()
            .iter()
            .all(|call| call.method == "GET" || call.endpoint == "graphql")
    );
}

#[test]
fn branch_warning_keeps_timestamp_until_tip_changes() {
    for sha in ["old", "different"] {
        let sandbox = Sandbox::new();
        let issue = report("ci[bot]", 1, sha);
        sandbox.branch_fixture(json!([issue]));
        successful(
            sandbox
                .command("stale-branch-cleanup")
                .env("DRY_RUN", "false")
                .output()
                .unwrap(),
        );
        let calls = sandbox.calls();
        let body = calls.last().unwrap().body["body"].as_str().unwrap();
        let state: Value = serde_json::from_str(
            body.split_once("<!-- personal-ci-stale-branches:")
                .unwrap()
                .1
                .split_once(" -->")
                .unwrap()
                .0,
        )
        .unwrap();
        let previous: Value = serde_json::from_str(
            issue["body"]
                .as_str()
                .unwrap()
                .split_once("<!-- personal-ci-stale-branches:")
                .unwrap()
                .1
                .split_once(" -->")
                .unwrap()
                .0,
        )
        .unwrap();
        assert_eq!(state["stale/topic"]["sha"], "old");
        assert_eq!(
            state["stale/topic"]["warned_at"] == previous["stale/topic"]["warned_at"],
            sha == "old"
        );
    }
}

fn pr_artifact_run(id: u64, conclusion: &str) -> Value {
    json!({"id": id, "workflow_id": 10, "run_number": id, "event": "pull_request",
        "status": "completed", "conclusion": conclusion, "pull_requests": [{"number": 7}],
        "created_at": "2026-10-29T12:00:00Z", "head_sha": "abc",
        "head_branch": "feature", "head_repository": {"id": 42}})
}

fn pr_build_fixture(sandbox: &Sandbox, include_image: bool) {
    let mut artifacts = vec![
        json!({"id":1,"name":"pr-source-tree","expired":false,"workflow_run":{"id":1}}),
        json!({"id":2,"name":"package","expired":false,"workflow_run":{"id":1}}),
    ];
    if include_image {
        artifacts.push(json!({"id":3,"name":"image","expired":false,"workflow_run":{"id":1}}));
    }

    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/1",
        0,
        pr_artifact_run(1, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/git/commits/abc",
        0,
        json!({"tree":{"sha":"tree"}}),
    );
    fs::write(
        sandbox.root().join("event"),
        r#"{"pull_request":{"number":7}}"#,
    )
    .unwrap();
}

#[test]
fn unchanged_build_reuses_a_complete_package_and_source_marker() {
    let sandbox = Sandbox::new();
    pr_build_fixture(&sandbox, true);
    successful(
        sandbox
            .command("pr-artifact-build")
            .env("GITHUB_EVENT_PATH", sandbox.root().join("event"))
            .env("GITHUB_RUN_ID", "2")
            .env("GITHUB_SHA", "abc")
            .env("REQUIRED_ARTIFACTS", r#"["package","image"]"#)
            .output()
            .unwrap(),
    );
    let output = fs::read_to_string(sandbox.root().join("output")).unwrap();
    assert!(output.contains("build=false"));
    assert!(output.contains("run-id=1"));
    assert!(output.contains(r#"artifacts=["package","image","pr-source-tree"]"#));
    assert!(sandbox.calls().iter().all(|call| call.method == "GET"));
}

#[test]
fn unchanged_build_rebuilds_when_any_required_output_is_missing() {
    let sandbox = Sandbox::new();
    pr_build_fixture(&sandbox, false);
    successful(
        sandbox
            .command("pr-artifact-build")
            .env("GITHUB_EVENT_PATH", sandbox.root().join("event"))
            .env("GITHUB_RUN_ID", "2")
            .env("GITHUB_SHA", "abc")
            .env("REQUIRED_ARTIFACTS", r#"["package","image"]"#)
            .output()
            .unwrap(),
    );
    let output = fs::read_to_string(sandbox.root().join("output")).unwrap();
    assert!(output.contains("build=true"));
    assert!(output.contains("run-id=0"));
}

#[test]
fn cleanup_deletes_older_runs_but_preserves_outputs_from_partial_reruns() {
    let sandbox = Sandbox::new();
    let artifacts: Vec<_> = [(1, 1), (2, 2), (3, 2)]
        .into_iter()
        .map(
            |(id, run)| json!({"id":id,"name":"package","expired":false,"workflow_run":{"id":run}}),
        )
        .collect();
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/1",
        0,
        pr_artifact_run(1, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    sandbox.response(
        "DELETE",
        "repos/example/project/actions/artifacts/1",
        0,
        Value::Null,
    );
    successful(sandbox.command("pr-artifact-cleanup").output().unwrap());
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .map(|call| call.endpoint)
        .collect();
    assert_eq!(deleted, ["repos/example/project/actions/artifacts/1"]);
}

#[test]
fn cleanup_paginates_and_continues_when_an_artifact_was_already_deleted() {
    let sandbox = Sandbox::new();
    let artifacts: Vec<_> = (1..=100)
        .map(|id| json!({"id":id,"name":"package","expired":false,"workflow_run":{"id":1}}))
        .collect();
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=2",
        0,
        json!({"artifacts":[{"id":101,"name":"package","expired":false,"workflow_run":{"id":2}}]}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/1",
        0,
        pr_artifact_run(1, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    for id in 1..=100 {
        let endpoint = format!("repos/example/project/actions/artifacts/{id}");
        sandbox.response(
            "DELETE",
            &endpoint,
            0,
            if id == 1 {
                json!({"status":"404"})
            } else {
                Value::Null
            },
        );
        if id == 1 {
            let key: String = endpoint.bytes().map(|byte| format!("{byte:02x}")).collect();
            fs::write(sandbox.root().join(format!("DELETE-{key}.exit")), "1").unwrap();
        }
    }

    successful(sandbox.command("pr-artifact-cleanup").output().unwrap());
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .collect();
    assert_eq!(deleted.len(), 100);
    assert!(deleted.iter().all(|call| !call.endpoint.ends_with("/101")));
}

#[test]
fn cleanup_removes_consumed_inputs_only_after_a_successful_final_upload() {
    let sandbox = Sandbox::new();
    let artifacts = json!([{"id":1,"name":"input-binary","expired":false,"workflow_run":{"id":2}},
        {"id":2,"name":"package","expired":false,"workflow_run":{"id":2}}]);
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    sandbox.response(
        "DELETE",
        "repos/example/project/actions/artifacts/1",
        0,
        Value::Null,
    );
    successful(
        sandbox
            .command("pr-artifact-cleanup")
            .env("TRANSIENT_PREFIX", "input-")
            .output()
            .unwrap(),
    );
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .map(|call| call.endpoint)
        .collect();
    assert_eq!(deleted, ["repos/example/project/actions/artifacts/1"]);
}
