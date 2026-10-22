#![cfg(test)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use personal_ci::release::scope_paths;
use serde_json::{Value, json};
use tempfile::TempDir;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const BINARY: &str = env!("CARGO_BIN_EXE_ci");

fn successful(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

struct Repository {
    temporary: TempDir,
    root: PathBuf,
    remote: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("source");
        let remote = temporary.path().join("remote.git");
        successful(
            &Command::new("git")
                .args(["init", "--bare", "--quiet", "--initial-branch=main"])
                .arg(&remote)
                .output()
                .unwrap(),
        );
        successful(
            &Command::new("git")
                .args(["init", "--quiet", "--initial-branch=main"])
                .arg(&root)
                .output()
                .unwrap(),
        );
        let repository = Self {
            temporary,
            root,
            remote,
        };
        for (key, value) in [
            ("user.name", "Release Test"),
            ("user.email", "release@example.com"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            repository.git(&["config", key, value]);
        }

        repository.git(&[
            "remote",
            "add",
            "origin",
            &format!("file://{}", repository.remote.display()),
        ]);
        repository
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        successful(&output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn commit(&self, path: &str, content: &str, message: &str) -> Value {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        self.git(&["add", "--", path.to_str().unwrap()]);
        self.git(&["commit", "--quiet", "-m", message]);
        json!({"hash":self.git(&["rev-parse", "HEAD"]), "message":message, "fixture":{"preserved":true}})
    }

    fn filter(&self, cwd: &Path, commits: &[Value], paths: &[&str]) -> Output {
        let mut child = Command::new(BINARY)
            .arg("release-filter")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        serde_json::to_writer(
            child.stdin.take().unwrap(),
            &json!({"cwd":cwd,"commits":commits,"paths":paths}),
        )
        .unwrap();
        child.wait_with_output().unwrap()
    }

    fn push(&self) {
        self.git(&["push", "--quiet", "origin", "HEAD:refs/heads/main"]);
    }

    fn tag(&self, tag: &str) {
        self.git(&["tag", tag]);
        self.git(&[
            "push",
            "--quiet",
            "origin",
            &format!("refs/tags/{tag}:refs/tags/{tag}"),
        ]);
    }

    fn remote_tags(&self) -> String {
        let output = Command::new("git")
            .arg("--git-dir")
            .arg(&self.remote)
            .arg("tag")
            .output()
            .unwrap();
        successful(&output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn config(&self) -> PathBuf {
        let directory = Path::new(ROOT).join(".github/actions/release/semantic-release");
        let installed = json!({
            "@semantic-release/commit-analyzer": directory.join("node_modules/@semantic-release/commit-analyzer/index.js"),
            "@semantic-release/release-notes-generator": directory.join("node_modules/@semantic-release/release-notes-generator/index.js")
        });
        let path = self.temporary.path().join("release.cjs");
        // Caller configuration preserves the generated Rust policy while omitting
        // the GitHub plugin, allowing the real engine to run entirely locally.
        let configuration = format!(
            "const settings = JSON.parse(require('node:fs').readFileSync(process.env.CI_RELEASE_SETTINGS, 'utf8'));\nconst installed = {installed};\nmodule.exports = {{repositoryUrl: {}, plugins: settings.plugins.filter(([name]) => name !== '@semantic-release/github').map(([name, options]) => [installed[name] || name, options])}};\n",
            json!(format!("file://{}", self.remote.display()))
        );
        fs::write(&path, configuration).unwrap();
        path
    }

    fn release(
        &self,
        cwd: &Path,
        config: &Path,
        inputs: &[(&str, &str)],
    ) -> (Output, BTreeMap<String, String>) {
        let output_file = self.temporary.path().join("github-output");
        fs::write(&output_file, "").unwrap();
        let output = release_command(cwd, &self.root)
            .current_dir(cwd)
            .env("CONFIG_FILE", config)
            .env("GITHUB_OUTPUT", &output_file)
            .env("SEMANTIC_RELEASE_TAG_PREFIX", "v")
            .env("SEMANTIC_RELEASE_COMMIT_PATHS", "")
            .env("FIRST_RELEASE_VERSION", "")
            .env("PRELAUNCH", "false")
            .env("EXTRA_PLUGINS", "")
            .envs(inputs.iter().copied())
            .output()
            .unwrap();
        let outputs = fs::read_to_string(output_file)
            .unwrap()
            .lines()
            .map(|line| {
                let (key, value) = line.split_once('=').unwrap();
                (key.to_owned(), value.to_owned())
            })
            .collect();
        (output, outputs)
    }
}

fn release_command(cwd: &Path, workspace: &Path) -> Command {
    let mut command = Command::new(BINARY);
    command
        .arg("release")
        .env("CI_ROOT", ROOT)
        .env("WORKING_DIRECTORY", cwd)
        .env("GITHUB_WORKSPACE", workspace)
        .env("DRY_RUN", "true")
        .env("CI", "true")
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REPOSITORY", "example/project")
        .env("SEMANTIC_RELEASE_BRANCHES", "main")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN");
    command
}

#[test]
fn scope_paths_normalize_boundaries_and_reject_absolute_or_parent_paths() {
    for (input, expected) in [
        (
            "./cmd/service/,\npkg/shared",
            vec!["cmd/service", "pkg/shared"],
        ),
        (" ./ , ./// ", vec![".", "."]),
        (".//cmd/service///", vec!["cmd/service"]),
        ("cmd\\service\\, go.work", vec!["cmd/service", "go.work"]),
        (",\n  ", vec![]),
    ] {
        assert_eq!(scope_paths(input).unwrap(), expected, "{input:?}");
    }

    for invalid in [
        "../service",
        "a/../service",
        "/service",
        "/",
        "///",
        "./../service",
        "cmd/one\r/two",
    ] {
        assert!(scope_paths(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn release_filter_uses_git_files_complete_segments_and_preserves_commit_objects() {
    let repository = Repository::new();
    let service = repository.commit(
        "cmd/service/main.rs",
        "service",
        "fix(service): repair source",
    );
    let other = repository.commit(
        "cmd/service-other/main.rs",
        "other",
        "feat!: replace unrelated API",
    );
    let root = repository.commit("go.work", "workspace", "fix: update workspace");
    let commits = [other, root.clone(), service.clone()];
    let cwd = repository.root.join("cmd/service");
    let output = repository.filter(&cwd, &commits, &["cmd/service"]);
    successful(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!([service])
    );
    let output = repository.filter(&cwd, &commits, &["go.work"]);
    successful(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!([root])
    );
    let output = repository.filter(&cwd, &commits, &["."]);
    successful(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!(commits)
    );
}

#[test]
fn release_filter_attributes_cross_module_renames_and_deletions_to_source() {
    let repository = Repository::new();
    repository.commit("cmd/service/main.rs", "service", "feat: add source");
    fs::create_dir_all(repository.root.join("cmd/other")).unwrap();
    repository.git(&["mv", "cmd/service/main.rs", "cmd/other/main.rs"]);
    repository.git(&["commit", "--quiet", "-m", "refactor: move service command"]);
    let moved = json!({"hash":repository.git(&["rev-parse","HEAD"]),"message":"refactor: move service command"});
    fs::create_dir_all(repository.root.join("cmd/service")).unwrap();
    let cwd = repository.root.join("cmd/service");
    for scope in ["cmd/service", "cmd/other"] {
        let output = repository.filter(&cwd, std::slice::from_ref(&moved), &[scope]);
        successful(&output);
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            json!([moved])
        );
    }

    repository.commit(
        "cmd/service/odd\nname.rs",
        "removed",
        "fix: add unusual filename",
    );
    repository.git(&["rm", "--", "cmd/service/odd\nname.rs"]);
    repository.git(&["commit", "--quiet", "-m", "fix: remove source"]);
    let removed =
        json!({"hash":repository.git(&["rev-parse","HEAD"]),"message":"fix: remove source"});
    fs::create_dir_all(&cwd).unwrap();
    let output = repository.filter(&cwd, std::slice::from_ref(&removed), &["cmd/service"]);
    successful(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!([removed])
    );
}

#[test]
fn release_filter_excludes_merge_messages_from_module_history() {
    let repository = Repository::new();
    repository.commit("README.md", "initial", "chore: initialize repository");
    repository.git(&["checkout", "--quiet", "-b", "authz"]);
    let authz = repository.commit(
        "cmd/authz/main.rs",
        "authz",
        "feat(authz): replace policies",
    );
    repository.git(&["checkout", "--quiet", "main"]);
    let service = repository.commit(
        "cmd/service/main.rs",
        "service",
        "fix(service): skip empty proxy",
    );
    repository.git(&[
        "merge",
        "--quiet",
        "--no-ff",
        "authz",
        "-m",
        "feat(authz)!: merge policy replacement",
    ]);
    let merge = json!({"hash":repository.git(&["rev-parse","HEAD"]),"message":"feat(authz)!: merge policy replacement"});
    let output = repository.filter(
        &repository.root.join("cmd/service"),
        &[merge, service.clone(), authz],
        &["cmd/service"],
    );
    successful(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!([service])
    );
}

#[test]
fn release_filter_rejects_empty_scopes_and_untrusted_commit_hashes() {
    let repository = Repository::new();
    let commit = repository.commit("source.txt", "source", "fix: source");
    let output = repository.filter(&repository.root, &[commit], &[]);
    assert!(!output.status.success());
    assert!(combined(&output).contains("requires at least one commit path"));
    for hash in [
        "not-a-hash",
        "--help",
        "deadbee;touch pwned",
        "$(touch pwned)",
    ] {
        let output = repository.filter(&repository.root, &[json!({"hash":hash})], &["."]);
        assert!(!output.status.success());
        assert!(combined(&output).contains("Invalid commit hash"));
    }

    assert!(!repository.root.join("pwned").exists());
}

#[test]
fn invalid_release_inputs_fail_before_npm_or_node() {
    static TOOLS: OnceLock<TempDir> = OnceLock::new();
    let tools = TOOLS.get_or_init(|| {
        let tools = tempfile::tempdir().unwrap();
        successful(
            &Command::new("rustc")
                .arg(Path::new(ROOT).join("tests/fixtures/release_tool.rs"))
                .arg("-o")
                .arg(tools.path().join("node"))
                .output()
                .unwrap(),
        );
        fs::copy(tools.path().join("node"), tools.path().join("npm")).unwrap();
        tools
    });
    let temporary = tempfile::tempdir().unwrap();
    let log = temporary.path().join("tool-log");
    for (name, values, diagnostic) in [
        (
            "EXTRA_PLUGINS",
            vec![
                "--help@1",
                "example@file:local",
                "example@https://example.com/package.tgz",
                "example",
                "example@",
                "example@1.0.0 --ignore-scripts",
                "@scope/pkg@../local",
            ],
            "Extra plugins must be public npm",
        ),
        (
            "FIRST_RELEASE_VERSION",
            vec!["not-semver", "v1.0.0", "1", "1.0.0\n"],
            "first-release-version must be a valid semver version",
        ),
        (
            "SEMANTIC_RELEASE_COMMIT_PATHS",
            vec!["/absolute", "../escape"],
            "repository-relative",
        ),
    ] {
        for value in values {
            let output = release_command(temporary.path(), temporary.path())
                .env("PATH", tools.path())
                .env("CI_RELEASE_TEST_TOOL_LOG", &log)
                .env("EXTRA_PLUGINS", "")
                .env("FIRST_RELEASE_VERSION", "")
                .env("SEMANTIC_RELEASE_COMMIT_PATHS", "")
                .env(name, value)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{name}={value:?}");
            assert!(
                combined(&output).contains(diagnostic),
                "{name}={value:?}: {}",
                combined(&output)
            );
            assert!(
                !log.exists(),
                "validation launched npm or Node for {name}={value:?}"
            );
        }
    }

    let paths = temporary.path().join("normalized-paths");
    let output = release_command(temporary.path(), temporary.path())
        .env("PATH", tools.path())
        .env("CI_RELEASE_TEST_TOOL_LOG", &log)
        .env("CI_RELEASE_TEST_PATHS_FILE", &paths)
        .env("EXTRA_PLUGINS", "")
        .env("FIRST_RELEASE_VERSION", "")
        .env(
            "SEMANTIC_RELEASE_COMMIT_PATHS",
            ".//cmd/service///, pkg\\shared\\",
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(91), "{}", combined(&output));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(paths).unwrap()).unwrap(),
        json!(["cmd/service", "pkg/shared"])
    );
}

#[test]
#[ignore = "requires npm ci --prefix .github/actions/release/semantic-release and Node 24+"]
fn semantic_release_dry_runs_initial_version_and_existing_tag_major_without_tokens() {
    let repository = Repository::new();
    repository.commit("source.txt", "one", "feat: add source");
    repository.push();
    let config = repository.config();
    let (output, outputs) = repository.release(
        &repository.root,
        &config,
        &[("FIRST_RELEASE_VERSION", "0.1.0")],
    );
    successful(&output);
    assert_eq!(outputs["version"], "0.1.0");
    assert_eq!(outputs["major_minor"], "0.1");
    assert_eq!(outputs["git_tag"], "v0.1.0");
    assert_eq!(outputs["release_strategy"], "initial");
    assert_eq!(outputs["release_published"], "false");
    assert!(repository.remote_tags().is_empty());
    assert!(repository.git(&["tag"]).is_empty());
    repository.tag("v1.0.0");
    repository.commit(
        "source.txt",
        "two",
        "feat!: replace source\n\nBREAKING CHANGE: remove old source",
    );
    repository.push();
    let (output, outputs) = repository.release(
        &repository.root,
        &config,
        &[("FIRST_RELEASE_VERSION", "0.1.0")],
    );
    successful(&output);
    assert_eq!(outputs["version"], "2.0.0");
    assert_eq!(outputs["git_tag"], "v2.0.0");
    assert_eq!(outputs["release_strategy"], "semantic");
    assert_eq!(outputs["release_published"], "false");
    assert!(combined(&output).contains("replace source"));
    assert_eq!(repository.remote_tags(), "v1.0.0");
    assert_eq!(repository.git(&["tag"]), "v1.0.0");
}

#[test]
#[ignore = "requires npm ci --prefix .github/actions/release/semantic-release and Node 24+"]
fn semantic_release_prelaunch_breaking_change_proposes_minor() {
    let repository = Repository::new();
    repository.commit("source.txt", "one", "chore: initial source");
    repository.tag("v0.3.0");
    repository.commit(
        "source.txt",
        "two",
        "feat!: replace source\n\nBREAKING CHANGE: remove old source",
    );
    repository.push();
    let config = repository.config();
    let (output, outputs) = repository.release(&repository.root, &config, &[("PRELAUNCH", "true")]);
    successful(&output);
    assert_eq!(outputs["version"], "0.4.0");
    assert_eq!(outputs["major_minor"], "0.4");
    assert_eq!(outputs["release_strategy"], "semantic");
    assert_eq!(outputs["release_published"], "false");
    assert_eq!(repository.remote_tags(), "v0.3.0");
}

#[test]
#[ignore = "requires npm ci --prefix .github/actions/release/semantic-release and Node 24+"]
fn semantic_release_scopes_analysis_and_notes_from_subdirectory() {
    let repository = Repository::new();
    repository.commit("cmd/service/source.txt", "one", "chore: initial service");
    repository.tag("cmd/service/v1.3.0");
    repository.commit(
        "cmd/authz/source.txt",
        "other",
        "feat(authz)!: replace policies\n\nBREAKING CHANGE: replace policies",
    );
    repository.commit(
        "cmd/service/source.txt",
        "two",
        "fix(service): skip empty proxy",
    );
    repository.push();
    let config = repository.config();
    let cwd = repository.root.join("cmd/service");
    let (output, outputs) = repository.release(
        &cwd,
        &config,
        &[("SEMANTIC_RELEASE_TAG_PREFIX", "cmd/service/v")],
    );
    successful(&output);
    assert_eq!(outputs["version"], "1.3.1");
    assert_eq!(outputs["git_tag"], "cmd/service/v1.3.1");
    assert_eq!(outputs["release_published"], "false");
    let text = combined(&output);
    assert!(text.contains("skip empty proxy"), "{text}");
    assert!(!text.contains("replace policies"), "{text}");
    assert_eq!(repository.remote_tags(), "cmd/service/v1.3.0");
}
