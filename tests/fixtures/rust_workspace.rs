use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use personal_ci::command::Failure;
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::TempDir;

pub(crate) const ROOT: &str = env!("CARGO_MANIFEST_DIR");

// Separate process preserves environment and working-directory isolation in tests.
#[test]
fn rust_check_process_entry() {
    if env::var_os("RUST_CHECK_PROCESS").is_none() {
        return;
    }

    if let Err(error) = personal_ci::rust_check::run() {
        eprintln!("{error:#}");
        std::process::exit(
            error
                .downcast_ref::<Failure>()
                .map_or(1, |failure| failure.0),
        );
    }
}

pub(crate) fn process(project: &Path) -> Command {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "rust_workspace::rust_check_process_entry",
            "--nocapture",
        ])
        .env("RUST_CHECK_PROCESS", "1")
        .env("WORKING_DIRECTORY", project)
        .env("SETUP_TOOLCHAIN", "false")
        .env("CHECKS", "fmt clippy test build")
        .env("ALL_FEATURES", "true")
        .env("BUILD_PROFILE", "dev")
        .env_remove("BUILD_TARGETS")
        .env("POLICY_DIRECTORY", "")
        .env("CI_ROOT", ROOT)
        .env_remove("CLIPPY_CONF_DIR");
    command
}

pub(crate) fn successful(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn diagnostic(output: &Output, expected: &str) {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(expected), "expected {expected:?}: {text}");
}

#[derive(Debug, Deserialize)]
pub(crate) struct Call {
    pub(crate) argv: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) clippy_conf_dir: Option<PathBuf>,
}

pub(crate) struct Fixture {
    pub(crate) temporary: TempDir,
    pub(crate) project: PathBuf,
    pub(crate) bin: PathBuf,
    pub(crate) metadata: Value,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        static CARGO: OnceLock<TempDir> = OnceLock::new();
        let executable = CARGO.get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            successful(
                &Command::new("rustc")
                    .arg(Path::new(ROOT).join("tests/fixtures/rust_check_cargo.rs"))
                    .arg("-o")
                    .arg(directory.path().join("cargo"))
                    .output()
                    .unwrap(),
            );
            directory
        });
        let temporary = tempfile::tempdir().unwrap();
        let project = temporary.path().join("workspace with spaces");
        fs::create_dir(&project).unwrap();
        let project = project.canonicalize().unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname = 'fixture'\n").unwrap();
        let bin = temporary.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::copy(executable.path().join("cargo"), bin.join("cargo")).unwrap();
        let metadata = json!({"workspace_members":["fixture"], "target_directory":temporary.path().join("custom target output"), "packages":[{"id":"fixture", "name":"fixture", "manifest_path":project.join("Cargo.toml"), "targets":[{"doctest":true}]}]});
        Self {
            temporary,
            project,
            bin,
            metadata,
        }
    }

    pub(crate) fn run(&self, inputs: &[(&str, &str)]) -> (Output, Vec<Call>) {
        let log = self.temporary.path().join("cargo.jsonl");
        if log.exists() {
            fs::remove_file(&log).unwrap();
        }

        let paths = env::join_paths(
            std::iter::once(self.bin.clone())
                .chain(env::split_paths(&env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let output = process(&self.project)
            .current_dir(self.temporary.path())
            .env("PATH", paths)
            .env("CARGO_LOG", &log)
            .env("CARGO_METADATA", self.metadata.to_string())
            .env("FAIL_CHECK", "")
            .env("FAIL_ARG", "")
            .envs(inputs.iter().copied())
            .output()
            .unwrap();
        let calls = fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (output, calls)
    }

    pub(crate) fn source(&self, path: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.project.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

pub(crate) fn names(calls: &[Call]) -> Vec<&str> {
    calls.iter().map(|call| call.argv[0].as_str()).collect()
}
