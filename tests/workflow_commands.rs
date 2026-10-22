#![cfg(test)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::{Value, json};
use tempfile::TempDir;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    directory: TempDir,
}

struct DiffCase {
    changed: &'static str,
    config: &'static str,
    operation: &'static str,
    expected: bool,
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
            .env("CI_ROOT", self.root())
            .env("GITHUB_OUTPUT", self.root().join("outputs"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("SCAN_ROOT", self.root())
            .env("ALLOW_TARGET", "false")
            .env("ALLOW_INHERIT", "false");
        command
    }

    fn output(&self, name: &str) -> String {
        fs::read_to_string(self.root().join("outputs"))
            .unwrap()
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(&format!("{name}=")).map(str::to_owned))
            .unwrap()
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
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn init(&self) {
        self.git(&["init", "--quiet"]);
        self.git(&["config", "user.name", "Workflow Tests"]);
        self.git(&["config", "user.email", "workflow-tests@example.invalid"]);
    }

    fn commit(&self) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "-m", "test: commit fixture"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn recorder(&self, operation: &str) -> Command {
        static RECORDER: OnceLock<TempDir> = OnceLock::new();
        let compiled = RECORDER.get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            let output = Command::new("rustc")
                .args(["--edition", "2024"])
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/workflow_commands.rs"),
                )
                .arg("-o")
                .arg(directory.path().join("recorder"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            directory
        });
        fs::create_dir_all(self.root().join("bin")).unwrap();
        fs::create_dir_all(self.root().join("recordings")).unwrap();
        for name in ["pm", "docker"] {
            fs::copy(
                compiled.path().join("recorder"),
                self.root().join("bin").join(name),
            )
            .unwrap();
        }

        let mut command = self.cli(operation);
        command
            .env("PM", self.root().join("bin/pm"))
            .env("PATH", self.root().join("bin"))
            .env("WORKFLOW_RECORDER_DIR", self.root().join("recordings"));
        command
    }

    fn calls(&self) -> Vec<Vec<String>> {
        let mut files: Vec<_> = fs::read_dir(self.root().join("recordings"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "args")
            })
            .collect();
        files.sort();
        files
            .into_iter()
            .map(|path| {
                fs::read_to_string(path)
                    .unwrap()
                    .split('\0')
                    .map(str::to_owned)
                    .collect()
            })
            .collect()
    }

    fn committed_actions(&self) -> String {
        self.init();
        for (path, contents) in [
            ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'\n"),
            ("Cargo.lock", "version = 4\n"),
            ("build.rs", "fn main() {}\n"),
            ("src/main.rs", "fn main() {}\n"),
            ("config/rust/lints.toml", "[rust]\n"),
            ("runtime-version", "1\n"),
            (".github/actions/run.sh", "#!/bin/sh\n"),
            (".github/actions/example/action.yaml", "name: Example\n"),
        ] {
            self.write(path, contents);
        }

        self.write(".github/workflows/test.yaml",&format!("jobs:\n  test:\n    steps:\n      - uses: kellen-miller/ci/.github/actions/example@{SHA}\n      - uses: actions/checkout@{SHA}\n"));
        self.commit()
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

fn failed(output: Output, expected: &str) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn changed_paths_uses_merge_base_and_preserves_complete_filenames() {
    let fixture = Fixture::new();
    fixture.init();
    for name in ["deleted.py", "renamed.py", "space and\nnewline.py"] {
        fixture.write(name, "base");
    }

    let base = fixture.commit();
    fixture.git(&["checkout", "--quiet", "-b", "feature"]);
    fs::remove_file(fixture.root().join("deleted.py")).unwrap();
    fs::rename(
        fixture.root().join("renamed.py"),
        fixture.root().join("new.txt"),
    )
    .unwrap();
    fixture.write("space and\nnewline.py", "changed");
    fixture.write("root.py", "new");
    fixture.write("nested/a/b.py", "new");
    let head = fixture.commit();
    fixture.git(&["checkout", "--quiet", "-b", "upstream", &base]);
    fixture.write("base-only.json", "upstream-only");
    let upstream = fixture.commit();
    let filters = json!({"deleted":["deleted.py"],"old_name":["renamed.py"],"new_name":["new.txt"],"unusual":["space and\nnewline.py"],"root_glob":["**/root.py"],"slash_glob":["nested/*.py"],"base_only":["base-only.json"],"missing":["absent/**"]});
    successful(
        fixture
            .cli("changed-paths")
            .env("EVENT_NAME", "pull_request")
            .env("FILTERS", filters.to_string())
            .env("BASE_SHA", upstream)
            .env("HEAD_SHA", head)
            .output()
            .unwrap(),
    );
    let selected: Value = serde_json::from_str(&fixture.output("changes")).unwrap();
    assert_eq!(
        selected,
        json!({"deleted":true,"old_name":true,"new_name":true,"unusual":true,"root_glob":true,"slash_glob":true,"base_only":false,"missing":false})
    );
}

#[test]
fn changed_paths_non_pr_selects_every_group_without_history() {
    for event in ["push", "workflow_dispatch", "schedule", "merge_group"] {
        let fixture = Fixture::new();
        successful(
            fixture
                .cli("changed-paths")
                .env("EVENT_NAME", event)
                .env(
                    "FILTERS",
                    json!({"rust":["src/**"],"root":["**/*.rs"]}).to_string(),
                )
                .env_remove("BASE_SHA")
                .env_remove("HEAD_SHA")
                .output()
                .unwrap(),
        );
        assert_eq!(
            serde_json::from_str::<Value>(&fixture.output("changes")).unwrap(),
            json!({"rust":true,"root":true})
        );
    }
}

#[test]
fn changed_paths_rejects_invalid_filters_for_every_event() {
    for event in [
        "pull_request",
        "push",
        "workflow_dispatch",
        "schedule",
        "merge_group",
    ] {
        for filters in [
            json!({}),
            json!([]),
            json!({"rust":[]}),
            json!({"rust":[""]}),
            json!({"rust":["!docs/**"]}),
            json!({"rust":"src/**"}),
            json!({"rust":[1]}),
            json!({"":["src/**"]}),
            json!({"rust":[null]}),
        ] {
            let fixture = Fixture::new();
            let output = fixture
                .cli("changed-paths")
                .env("EVENT_NAME", event)
                .env("FILTERS", filters.to_string())
                .env("BASE_SHA", "")
                .env("HEAD_SHA", "")
                .output()
                .unwrap();
            assert!(!output.status.success(), "event={event}; filters={filters}");
            assert!(!fixture.root().join("outputs").exists());
        }
    }

    for reference in ["", "missing"] {
        let fixture = Fixture::new();
        fixture.init();
        fixture.write("README.md", "base");
        fixture.commit();
        assert!(
            !fixture
                .cli("changed-paths")
                .env("EVENT_NAME", "pull_request")
                .env("FILTERS", json!({"rust":["src/**"]}).to_string())
                .env("BASE_SHA", reference)
                .env("HEAD_SHA", reference)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn workflow_security_parses_pins_checkout_and_dependency_boundaries() {
    let fixture = Fixture::new();
    fixture.write(".github/workflows/test.yaml",&format!("on: pull_request\n# pull_request_target and secrets: inherit are documentation\njobs:\n  test:\n    steps:\n      - uses: actions/checkout@{SHA}\n        with:\n          persist-credentials: false\n      - uses: actions/checkout@{SHA}\n        with:\n          persist-credentials: 'false'\n      - uses: ./.github/actions/example\n      - uses: docker://alpine@sha256:{}\n", "a".repeat(64)));
    fixture.write(
        ".github/actions/example/action.yml",
        "name: Example\nruns:\n  using: composite\n  steps: []\n",
    );
    for dependency in ["node_modules", ".venv", ".git"] {
        fixture.write(
            &format!(".github/actions/release/{dependency}/invalid.yaml"),
            "invalid: [yaml\n",
        );
    }

    assert!(
        successful(fixture.cli("workflow-security").output().unwrap())
            .contains("Validated 2 workflow/action files")
    );
}

#[test]
fn workflow_security_rejects_unpinned_actions_and_checkout_credentials() {
    for reference in [
        "example/ci/.github/workflows/lint.yaml@main",
        "actions/cache@v4",
        "actions/cache@abc",
        "actions/cache@AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        let fixture = Fixture::new();
        fixture.write(
            ".github/workflows/test.yaml",
            &format!("jobs:\n  check:\n    uses: {reference}\n"),
        );
        failed(
            fixture.cli("workflow-security").output().unwrap(),
            "full commit SHA",
        );
    }

    for setting in [
        "",
        "        with:\n          persist-credentials: true\n",
        "        with:\n          persist-credentials: 'true'\n",
    ] {
        let fixture = Fixture::new();
        fixture.write(".github/workflows/test.yaml",&format!("jobs:\n  check:\n    steps:\n      - name: Checkout\n        uses: actions/checkout@{SHA}\n{setting}"));
        failed(
            fixture.cli("workflow-security").output().unwrap(),
            "persist-credentials: false",
        );
    }
}

#[test]
fn workflow_security_requires_target_and_inheritance_opt_ins() {
    for trigger in [
        "on: pull_request_target\n",
        "on: [push, pull_request_target]\n",
        "on:\n  pull_request_target:\n",
    ] {
        let fixture = Fixture::new();
        fixture.write(".github/workflows/test.yaml",&format!("{trigger}jobs:\n  check:\n    uses: example/ci/.github/workflows/lint.yaml@{SHA}\n    secrets:\n      inherit\n"));
        let output = fixture.cli("workflow-security").output().unwrap();
        failed(output, "pull_request_target requires explicit opt-in");
        failed(
            fixture
                .cli("workflow-security")
                .arg("--allow-pull-request-target")
                .output()
                .unwrap(),
            "named secrets",
        );
        successful(
            fixture
                .cli("workflow-security")
                .arg("--allow-pull-request-target")
                .arg("--allow-secret-inheritance")
                .output()
                .unwrap(),
        );
        successful(
            fixture
                .cli("workflow-security")
                .env("ALLOW_TARGET", "true")
                .env("ALLOW_INHERIT", "true")
                .output()
                .unwrap(),
        );
    }
}

#[test]
fn workflow_security_rejects_docker_tags_bad_yaml_and_empty_roots() {
    for reference in ["alpine:latest", "alpine@sha256:abc"] {
        let fixture = Fixture::new();
        fixture.write(
            ".github/actions/test/action.yaml",
            &format!("runs:\n  steps:\n    - uses: docker://{reference}\n"),
        );
        failed(
            fixture.cli("workflow-security").output().unwrap(),
            "Docker action must use a SHA-256 digest",
        );
    }

    let fixture = Fixture::new();
    fixture.write(".github/workflows/test.yaml", "jobs: [unterminated\n");
    failed(
        fixture.cli("workflow-security").output().unwrap(),
        "invalid YAML",
    );
    failed(
        Fixture::new().cli("workflow-security").output().unwrap(),
        "no workflow or action YAML",
    );
}

#[test]
fn renovate_changes_gates_literal_paths_deletions_and_caller_workflows() {
    let cases = [
        DiffCase {
            changed: "README.md",
            config: "renovate.json",
            operation: "edit",
            expected: false,
        },
        DiffCase {
            changed: "renovate.json",
            config: "renovate.json",
            operation: "edit",
            expected: true,
        },
        DiffCase {
            changed: "config/custom.json5",
            config: "config/custom.json5",
            operation: "edit",
            expected: true,
        },
        DiffCase {
            changed: "default.json",
            config: "renovate.json",
            operation: "edit",
            expected: true,
        },
        DiffCase {
            changed: ".github/workflows/caller.yml",
            config: "renovate.json",
            operation: "edit",
            expected: true,
        },
        DiffCase {
            changed: "renovate.json",
            config: "renovate.json",
            operation: "delete",
            expected: true,
        },
        DiffCase {
            changed: "renovate.json",
            config: "renovate.json",
            operation: "rename",
            expected: true,
        },
        DiffCase {
            changed: "renovate.json",
            config: "renovate.json",
            operation: "base-only",
            expected: false,
        },
        DiffCase {
            changed: "config/[prod].json",
            config: "config/[prod].json",
            operation: "edit",
            expected: true,
        },
        DiffCase {
            changed: "config/p.json",
            config: "config/[prod].json",
            operation: "edit",
            expected: false,
        },
    ];
    for case in cases {
        let fixture = Fixture::new();
        fixture.init();
        fixture.write(case.changed, "base");
        let mut base = fixture.commit();
        match case.operation {
            "delete" => fs::remove_file(fixture.root().join(case.changed)).unwrap(),
            "rename" => fs::rename(
                fixture.root().join(case.changed),
                fixture.root().join("old.json"),
            )
            .unwrap(),
            _ => fixture.write(case.changed, "changed"),
        }

        let mut head = fixture.commit();
        if case.operation == "base-only" {
            std::mem::swap(&mut base, &mut head);
        }

        successful(
            fixture
                .cli("renovate-changes")
                .env("EVENT_NAME", "pull_request")
                .env("BASE_SHA", base)
                .env("HEAD_SHA", head)
                .env("CONFIG_FILE", case.config)
                .output()
                .unwrap(),
        );
        assert_eq!(
            fixture.output("validate"),
            case.expected.to_string(),
            "{} {}",
            case.changed,
            case.operation
        );
    }

    let fixture = Fixture::new();
    successful(
        fixture
            .cli("renovate-changes")
            .env("EVENT_NAME", "push")
            .env_remove("BASE_SHA")
            .env_remove("HEAD_SHA")
            .env_remove("CONFIG_FILE")
            .output()
            .unwrap(),
    );
    assert_eq!(fixture.output("validate"), "true");
}

#[test]
fn pin_actions_rejects_dirty_or_untracked_runtime_inputs_before_writing() {
    for path in [
        "build.rs",
        "src/main.rs",
        "src/new.rs",
        "config/rust/lints.toml",
        "config/new.toml",
        "Cargo.toml",
        "Cargo.lock",
        "runtime-version",
        ".github/actions/example/action.yaml",
    ] {
        let fixture = Fixture::new();
        let commit = fixture.committed_actions();
        let workflow =
            fs::read_to_string(fixture.root().join(".github/workflows/test.yaml")).unwrap();
        fixture.write(path, "uncommitted runtime input\n");
        failed(
            fixture.cli("pin-actions").arg(commit).output().unwrap(),
            "Commit action, Rust and policy changes before pinning workflows",
        );
        assert_eq!(
            fs::read_to_string(fixture.root().join(".github/workflows/test.yaml")).unwrap(),
            workflow
        );
    }
}

#[test]
fn pin_actions_selects_committed_snapshot_and_preserves_other_references() {
    let fixture = Fixture::new();
    let first = fixture.committed_actions();
    fixture.write("src/main.rs", "fn main() { println!(\"new\"); }\n");
    let second = fixture.commit();
    failed(
        fixture.cli("pin-actions").arg(&first).output().unwrap(),
        "Commit action, Rust and policy changes before pinning workflows",
    );
    successful(fixture.cli("pin-actions").arg(&second).output().unwrap());
    let workflow = fs::read_to_string(fixture.root().join(".github/workflows/test.yaml")).unwrap();
    assert!(workflow.contains(&format!(
        "kellen-miller/ci/.github/actions/example@{second}"
    )));
    assert!(workflow.contains(&format!("actions/checkout@{SHA}")));
    fixture.write(
        ".github/workflows/test.yaml",
        &format!("jobs:\n  test:\n    uses: kellen-miller/ci/.github/actions/missing@{SHA}\n"),
    );
    let before = fs::read_to_string(fixture.root().join(".github/workflows/test.yaml")).unwrap();
    assert!(
        !fixture
            .cli("pin-actions")
            .arg(second)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(fixture.root().join(".github/workflows/test.yaml")).unwrap(),
        before
    );
}

#[test]
fn platform_matrix_deduplicates_and_rejects_artifact_collisions() {
    let fixture = Fixture::new();
    successful(
        fixture
            .cli("platform-matrix")
            .env("PLATFORMS", "linux/amd64, linux/arm64,linux/amd64")
            .output()
            .unwrap(),
    );
    let matrix: Value = serde_json::from_str(&fixture.output("matrix")).unwrap();
    assert_eq!(
        matrix,
        json!({"include":[{"platform":"linux/amd64","suffix":"linux-amd64","runner_label":"ubuntu-latest"},{"platform":"linux/arm64","suffix":"linux-arm64","runner_label":"ubuntu-24.04-arm"}]})
    );
    successful(
        fixture
            .cli("platform-matrix")
            .env("PLATFORMS", "")
            .output()
            .unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&fixture.output("matrix")).unwrap()["include"][0]["suffix"],
        "default"
    );
    failed(
        fixture
            .cli("platform-matrix")
            .env("PLATFORMS", "linux/arm64,linux-arm64")
            .output()
            .unwrap(),
        "platform artifact names collide",
    );
    failed(
        fixture
            .cli("platform-matrix")
            .env("PLATFORMS", ",,,")
            .output()
            .unwrap(),
        "no build platforms selected",
    );
}

#[test]
fn manifest_rejects_invalid_digest_artifacts_before_child_execution() {
    for invalid in ["empty", "bad-name", "directory", "symlink"] {
        let fixture = Fixture::new();
        fs::create_dir(fixture.root().join("digests")).unwrap();
        match invalid {
            "bad-name" => fixture.write("digests/not-a-digest", ""),
            "directory" => {
                fs::create_dir(fixture.root().join("digests").join("a".repeat(64))).unwrap()
            }

            "symlink" => {
                fixture.write("outside", "");
                std::os::unix::fs::symlink(
                    fixture.root().join("outside"),
                    fixture.root().join("digests").join("a".repeat(64)),
                )
                .unwrap();
            }

            _ => {}
        }

        let output = fixture
            .recorder("docker-manifest")
            .env("IMAGE", "ghcr.io/example/project")
            .env("DIGEST_DIR", fixture.root().join("digests"))
            .env("TAGS", "ghcr.io/example/project:v1")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{invalid}");
        assert!(fixture.calls().is_empty());
    }
}

#[test]
fn pages_build_validates_assignments_and_passes_literal_values() {
    for assignment in ["MISSING_EQUALS", "1BAD=value", "BAD-NAME=value", "=value"] {
        let fixture = Fixture::new();
        failed(
            fixture
                .recorder("pages-build")
                .env("SCRIPT", "build")
                .env("BUILD_ENV", assignment)
                .output()
                .unwrap(),
            "Invalid build environment assignment",
        );
        assert!(fixture.calls().is_empty());
    }

    let fixture = Fixture::new();
    successful(
        fixture
            .recorder("pages-build")
            .env("SCRIPT", "build with spaces")
            .env(
                "BUILD_ENV",
                "SITE=https://example.invalid?a=b\n_BUILD1=$(touch should-not-exist)\n",
            )
            .env("WORKFLOW_RECORDER_KEYS", "SITE,_BUILD1")
            .output()
            .unwrap(),
    );
    assert_eq!(fixture.calls(), vec![vec!["run", "build with spaces"]]);
    let values: BTreeMap<_, _> = fs::read_to_string(fixture.root().join("recordings/0000.env"))
        .unwrap()
        .split('\0')
        .map(|assignment| assignment.split_once('=').unwrap())
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    assert_eq!(values["SITE"], "https://example.invalid?a=b");
    assert_eq!(values["_BUILD1"], "$(touch should-not-exist)");
    assert!(!fixture.root().join("should-not-exist").exists());
}

#[test]
fn node_checks_requires_a_script_and_preserves_execution_order_and_status() {
    let fixture = Fixture::new();
    failed(
        fixture
            .recorder("node-checks")
            .env("FORMAT_SCRIPT", "")
            .env("CHECK_SCRIPT", "")
            .env("LINT_SCRIPT", "")
            .output()
            .unwrap(),
        "At least one format, check or lint script is required",
    );
    assert!(fixture.calls().is_empty());
    successful(
        fixture
            .recorder("node-checks")
            .env("FORMAT_SCRIPT", "format")
            .env("CHECK_SCRIPT", "")
            .env("LINT_SCRIPT", "lint spaces")
            .output()
            .unwrap(),
    );
    assert_eq!(
        fixture.calls(),
        vec![vec!["run", "format"], vec!["run", "lint spaces"]]
    );
    let output = fixture
        .recorder("node-checks")
        .env("FORMAT_SCRIPT", "format")
        .env("CHECK_SCRIPT", "check")
        .env("LINT_SCRIPT", "lint")
        .env("WORKFLOW_RECORDER_STATUS", "9")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(9));
    assert_eq!(fixture.calls().len(), 3);
}
