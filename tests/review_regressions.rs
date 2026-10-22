#![cfg(test)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

const PIN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    directory: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn cli(&self, operation: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ci"));
        command
            .arg(operation)
            .current_dir(self.root())
            .env("SCAN_ROOT", self.root())
            .env("ALLOW_TARGET", "false")
            .env("ALLOW_INHERIT", "false")
            .env("GITHUB_OUTPUT", self.root().join("outputs"))
            .env_remove("GITHUB_STEP_SUMMARY")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn git(&self, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(self.root())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn renovate(&self) -> Command {
        self.git(&["init", "--quiet"]);
        self.git(&["config", "user.name", "Review Tests"]);
        self.git(&["config", "user.email", "review-tests@example.invalid"]);
        self.write("renovate.json", "{}\n");
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "-m", "test: commit fixture"]);
        let sha = self.git(&["rev-parse", "HEAD"]);
        let mut command = self.cli("renovate-changes");
        command
            .env("EVENT_NAME", "pull_request")
            .env("BASE_SHA", &sha)
            .env("HEAD_SHA", sha)
            .env("CONFIG_FILE", "renovate.json");
        command
    }
}

fn succeeded(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn workflow_security_rejects_empty_and_nonmapping_yaml() {
    for contents in ["", "null\n", "42\n", "workflow\n", "[]\n", "- jobs: {}\n"] {
        let fixture = Fixture::new();
        fixture.write(".github/workflows/test.yaml", contents);
        let output = fixture.cli("workflow-security").output().unwrap();
        assert!(!output.status.success(), "accepted {contents:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(".github/workflows/test.yaml: expected a YAML mapping"),
            "{error}"
        );
    }
}

#[test]
#[cfg(unix)]
fn workflow_security_reads_symlinked_yaml_without_following_directory_cycles() {
    let fixture = Fixture::new();
    fixture.write(
        ".github/workflows/test.yaml",
        "on: pull_request\njobs: {}\n",
    );
    fixture.write(
        "payload.yaml",
        "jobs:\n  cache:\n    uses: actions/cache@main\n",
    );
    fs::create_dir_all(fixture.root().join(".github/actions/example")).unwrap();
    std::os::unix::fs::symlink(
        fixture.root().join("payload.yaml"),
        fixture.root().join(".github/actions/example/action.yaml"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        fixture.root().join(".github/actions"),
        fixture.root().join(".github/actions/cycle"),
    )
    .unwrap();
    let output = fixture.cli("workflow-security").output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains(".github/actions/example/action.yaml"),
        "{error}"
    );
    assert!(error.contains("full commit SHA"), "{error}");
    fixture.write(
        "payload.yaml",
        &format!("jobs:\n  cache:\n    uses: actions/cache@{PIN}\n"),
    );
    assert!(
        succeeded(fixture.cli("workflow-security").output().unwrap())
            .contains("Validated 2 workflow/action files")
    );
}

#[test]
#[cfg(unix)]
fn workflow_security_reports_broken_yaml_symlinks() {
    let fixture = Fixture::new();
    fixture.write(
        ".github/workflows/test.yaml",
        "on: pull_request\njobs: {}\n",
    );
    std::os::unix::fs::symlink(
        fixture.root().join("absent.yaml"),
        fixture.root().join(".github/workflows/broken.yaml"),
    )
    .unwrap();
    let output = fixture.cli("workflow-security").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("broken.yaml"));
}

#[test]
#[cfg(unix)]
fn workflow_security_reports_unreadable_yaml_link_targets() {
    let fixture = Fixture::new();
    fixture.write(
        ".github/workflows/test.yaml",
        "on: pull_request\njobs: {}\n",
    );
    fs::write(fixture.root().join("payload.yaml"), [0xff]).unwrap();
    std::os::unix::fs::symlink(
        fixture.root().join("payload.yaml"),
        fixture.root().join(".github/workflows/unreadable.yaml"),
    )
    .unwrap();
    let output = fixture.cli("workflow-security").output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("read workflow YAML:"), "{error}");
    assert!(error.contains("unreadable.yaml"), "{error}");
}

#[test]
fn renovate_unchanged_pr_appends_summary_and_keeps_local_summary_optional() {
    let fixture = Fixture::new();
    let mut command = fixture.renovate();
    fixture.write("summary", "Earlier result.\n");
    succeeded(
        command
            .env("GITHUB_STEP_SUMMARY", fixture.root().join("summary"))
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(fixture.root().join("summary")).unwrap(),
        "Earlier result.\nRenovate config unchanged; skipping validation.\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root().join("outputs")).unwrap(),
        "validate=false\n"
    );
    succeeded(command.env_remove("GITHUB_STEP_SUMMARY").output().unwrap());
}

#[test]
fn renovate_non_pr_keeps_summary_unchanged() {
    let fixture = Fixture::new();
    fixture.write("summary", "Earlier result.\n");
    succeeded(
        fixture
            .cli("renovate-changes")
            .env("EVENT_NAME", "push")
            .env("GITHUB_STEP_SUMMARY", fixture.root().join("summary"))
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(fixture.root().join("summary")).unwrap(),
        "Earlier result.\n"
    );
}

#[test]
fn provenance_preserves_build_dependencies_metadata_and_empty_byproducts() {
    for workflow_sha in [None, Some(""), Some("workflow-commit")] {
        let fixture = Fixture::new();
        let mut command = fixture.cli("provenance");
        command
            .env("REPOSITORY", "https://github.com/example/project")
            .env("REF", "refs/heads/main")
            .env("SHA", "source-commit")
            .env(
                "WORKFLOW_REF",
                "example/ci/.github/workflows/publish.yaml@refs/heads/main",
            )
            .env("BUILD_TYPE", "https://example.invalid/build")
            .env(
                "INVOCATION_ID",
                "https://github.com/example/project/actions/runs/123",
            );
        if let Some(sha) = workflow_sha {
            command.env("WORKFLOW_SHA", sha);
        } else {
            command.env_remove("WORKFLOW_SHA");
        }

        succeeded(command.output().unwrap());
        let predicate: Value =
            serde_json::from_slice(&fs::read(fixture.root().join("slsa-provenance.json")).unwrap())
                .unwrap();
        assert_eq!(
            predicate,
            json!({"buildDefinition":{"buildType":"https://example.invalid/build","externalParameters":{"repository":"https://github.com/example/project","ref":"refs/heads/main","workflowRef":"example/ci/.github/workflows/publish.yaml@refs/heads/main"},"internalParameters":{},"resolvedDependencies":[{"uri":"git+https://github.com/example/project@refs/heads/main","digest":{"gitCommit":"source-commit"}},{"uri":"example/ci/.github/workflows/publish.yaml@refs/heads/main","digest":{"gitCommit":workflow_sha.filter(|sha|!sha.is_empty()).unwrap_or("source-commit")}}]},"runDetails":{"builder":{"id":"https://github.com/actions/runner"},"metadata":{"invocationId":"https://github.com/example/project/actions/runs/123"},"byproducts":[]}})
        );
    }
}
