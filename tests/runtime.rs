#![cfg(test)]

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use personal_ci::packaging;

#[test]
fn released_runtime_checks_its_action_source_identity_before_side_effects() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = TempDir::new().unwrap();
    let output = temporary.path().join("outputs");
    let valid = Command::new(env!("CARGO_BIN_EXE_ci"))
        .arg("changed-paths")
        .env("CI_ROOT", root)
        .env("CI_VERIFY_SOURCE", "true")
        .env("FILTERS", r#"{"all":["**"]}"#)
        .env("EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_OUTPUT", &output)
        .output()
        .unwrap();
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        "changes={\"all\":true}\n"
    );

    let stale = temporary.path().join("stale");
    fs::create_dir_all(stale.join("src")).unwrap();
    for file in ["Cargo.toml", "Cargo.lock", "build.rs"] {
        fs::copy(root.join(file), stale.join(file)).unwrap();
    }

    for entry in walkdir::WalkDir::new(root.join("src")) {
        let entry = entry.unwrap();
        let destination = stale.join(entry.path().strip_prefix(root).unwrap());
        if entry.file_type().is_dir() {
            fs::create_dir_all(destination).unwrap();
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }

    fs::write(
        stale.join("src/new_operation.rs"),
        "// changed action implementation\n",
    )
    .unwrap();
    fs::remove_file(&output).unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_ci"))
        .arg("changed-paths")
        .env("CI_ROOT", stale)
        .env("CI_VERIFY_SOURCE", "true")
        .env("FILTERS", r#"{"all":["**"]}"#)
        .env("EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_OUTPUT", &output)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("does not match action sources"));
    assert!(!output.exists());
}

struct RuntimeFixture {
    runner_temp: TempDir,
    assets: TempDir,
    tools: TempDir,
    no_curl_tools: TempDir,
    version: String,
    runner_os: String,
    runner_arch: String,
    target: String,
    release_url: String,
    curl_log: PathBuf,
    runtime_log: PathBuf,
    recorder: PathBuf,
    bash: PathBuf,
}

impl RuntimeFixture {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let version = fs::read_to_string(root.join("runtime-version"))
            .unwrap()
            .trim()
            .to_owned();
        let (runner_os, runner_arch, target) = runtime_platform();
        let release_url =
            format!("https://github.com/kellen-miller/ci/releases/download/v{version}");
        let runner_temp = tempfile::tempdir().unwrap();
        let assets = tempfile::tempdir().unwrap();
        let tools = tempfile::tempdir().unwrap();
        let no_curl_tools = tempfile::tempdir().unwrap();
        let curl_log = runner_temp.path().join("curl.log");
        let runtime_log = runner_temp.path().join("runtime.log");
        let recorder = compile_fixture(
            &root.join("tests/fixtures/local_tools_recorder.rs"),
            &runner_temp.path().join(executable_name("runtime-recorder")),
        );
        let download_fixture = compile_fixture(
            &root.join("tests/fixtures/runtime_download.rs"),
            &runner_temp.path().join(executable_name("runtime-download")),
        );
        copy_fixture_tool(&download_fixture, &tools, "curl");
        copy_fixture_tool(&download_fixture, &tools, "dirname");
        copy_fixture_tool(&download_fixture, &no_curl_tools, "dirname");
        let bash = locate_program("bash");
        Self {
            runner_temp,
            assets,
            tools,
            no_curl_tools,
            version,
            runner_os: runner_os.to_owned(),
            runner_arch: runner_arch.to_owned(),
            target: target.to_owned(),
            release_url,
            curl_log,
            runtime_log,
            recorder,
            bash,
        }
    }

    fn package_recorder(&self) -> PathBuf {
        packaging::package(&self.target, &self.recorder, self.assets.path()).unwrap();
        self.assets.path().join(self.archive_name())
    }

    fn archive_name(&self) -> String {
        format!("ci-{}.tar.gz", self.target)
    }

    fn cache_directory(&self) -> PathBuf {
        self.runner_temp
            .path()
            .join("personal-ci")
            .join(&self.version)
            .join(&self.target)
    }

    fn bootstrap(&self, args: &[&str], binary: Option<&Path>, include_system_path: bool) -> Output {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut command = Command::new(&self.bash);
        command
            .arg(root.join(".github/actions/run.sh"))
            .args(args)
            .current_dir(root)
            .env("RUNNER_TEMP", self.runner_temp.path())
            .env("RUNNER_OS", &self.runner_os)
            .env("RUNNER_ARCH", &self.runner_arch)
            .env("RUNTIME_EXPECTED_BASE", &self.release_url)
            .env("RUNTIME_EXPECTED_OUTPUT", self.cache_directory())
            .env("RUNTIME_ASSETS", self.assets.path())
            .env("RUNTIME_CURL_LOG", &self.curl_log)
            .env("RECORDER_LOG", &self.runtime_log)
            .env(
                "RECORDER_STDOUT",
                format!("fixture-runtime-{}\n", self.version),
            )
            .env_remove("CI_BINARY");
        if let Some(binary) = binary {
            command.env("CI_BINARY", binary);
        }

        let path = if include_system_path {
            let mut directories =
                env::split_paths(&env::var_os("PATH").unwrap()).collect::<Vec<_>>();
            directories.insert(0, self.tools.path().to_path_buf());
            env::join_paths(directories).unwrap()
        } else {
            env::join_paths([self.no_curl_tools.path()]).unwrap()
        };

        command.env("PATH", path);
        command.output().unwrap()
    }
}

#[test]
fn package_archive_is_deterministic_named_checksummed_and_executable() {
    let temp = tempfile::tempdir().unwrap();
    let recorder = compile_fixture(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/local_tools_recorder.rs"),
        &temp.path().join(executable_name("recorder")),
    );
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let target = "x86_64-unknown-linux-musl";
    packaging::package(target, &recorder, &first).unwrap();
    packaging::package(target, &recorder, &second).unwrap();

    let archive_name = format!("ci-{target}.tar.gz");
    let first_archive = fs::read(first.join(&archive_name)).unwrap();
    let second_archive = fs::read(second.join(&archive_name)).unwrap();
    assert_eq!(first_archive, second_archive);
    let digest = hex::encode(Sha256::digest(&first_archive));
    assert_eq!(
        fs::read_to_string(first.join(format!("{archive_name}.sha256"))).unwrap(),
        format!("{digest}  {archive_name}\n")
    );
    assert_eq!(
        fs::read(first.join(format!("{archive_name}.sha256"))).unwrap(),
        fs::read(second.join(format!("{archive_name}.sha256"))).unwrap()
    );

    let mut archive = tar::Archive::new(GzDecoder::new(first_archive.as_slice()));
    let mut entries = archive.entries().unwrap();
    let mut entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new("ci"));
    assert_eq!(entry.header().mode().unwrap(), 0o755);
    let mut contents = Vec::new();
    entry.read_to_end(&mut contents).unwrap();
    assert_eq!(contents, fs::read(recorder).unwrap());
    assert!(entries.next().is_none());
}

#[test]
fn windows_package_uses_the_exe_member_name() {
    let temp = tempfile::tempdir().unwrap();
    let recorder = compile_fixture(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/local_tools_recorder.rs"),
        &temp.path().join(executable_name("recorder")),
    );
    let output = temp.path().join("package");
    let target = "x86_64-pc-windows-msvc";
    packaging::package(target, &recorder, &output).unwrap();
    let archive_path = output.join(format!("ci-{target}.tar.gz"));
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(archive_path).unwrap()));
    let mut entries = archive.entries().unwrap();
    assert_eq!(
        entries.next().unwrap().unwrap().path().unwrap(),
        Path::new("ci.exe")
    );
}

#[test]
fn bootstrap_downloads_versioned_platform_assets_and_executes_after_checksum() {
    let fixture = RuntimeFixture::new();
    let archive = fixture.package_recorder();
    let result = fixture.bootstrap(&["fixture-argument"], None, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains(&format!("fixture-runtime-{}", fixture.version))
    );
    assert_eq!(
        fs::read_to_string(&fixture.runtime_log).unwrap(),
        "fixture-argument\n"
    );

    let cached_archive = fixture.cache_directory().join(fixture.archive_name());
    assert_eq!(
        fs::read(cached_archive).unwrap(),
        fs::read(archive).unwrap()
    );
    let calls = fs::read_to_string(&fixture.curl_log).unwrap();
    assert!(calls.contains(&format!(
        "{}/{}\t",
        fixture.release_url,
        fixture.archive_name()
    )));
    assert!(calls.contains(&format!(
        "{}/{}.sha256\t",
        fixture.release_url,
        fixture.archive_name()
    )));
    assert_eq!(calls.lines().count(), 2);
}

#[test]
fn incorrect_or_missing_checksum_record_stops_before_runtime_execution() {
    let fixture = RuntimeFixture::new();
    fixture.package_recorder();
    let checksum = fixture
        .assets
        .path()
        .join(format!("{}.sha256", fixture.archive_name()));
    fs::write(
        &checksum,
        format!("{}  {}\n", "0".repeat(64), fixture.archive_name()),
    )
    .unwrap();
    let wrong = fixture.bootstrap(&["should-not-run"], None, true);
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("CI runtime checksum mismatch"));
    assert!(!fixture.runtime_log.exists());
    assert!(
        !fixture
            .cache_directory()
            .join(fixture.archive_name())
            .exists()
    );
    assert!(
        !fixture
            .cache_directory()
            .join(format!("{}.sha256", fixture.archive_name()))
            .exists()
    );

    fs::write(
        &checksum,
        format!("{}  other-archive.tar.gz\n", "a".repeat(64)),
    )
    .unwrap();
    let missing = fixture.bootstrap(&["should-not-run"], None, true);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("Invalid CI runtime checksum record")
    );
    assert!(!fixture.runtime_log.exists());
}

#[test]
fn corrupt_versioned_cache_is_rejected_and_removed_without_download() {
    let fixture = RuntimeFixture::new();
    let good_archive = fixture.package_recorder();
    let archive_path = fixture.cache_directory().join(fixture.archive_name());
    fs::create_dir_all(fixture.cache_directory()).unwrap();
    let mut corrupted = fs::read(good_archive).unwrap();
    let corrupt_index = corrupted.len() / 2;
    corrupted[corrupt_index] ^= 0xff;
    fs::write(&archive_path, corrupted).unwrap();
    fs::copy(
        fixture
            .assets
            .path()
            .join(format!("{}.sha256", fixture.archive_name())),
        fixture
            .cache_directory()
            .join(format!("{}.sha256", fixture.archive_name())),
    )
    .unwrap();

    let result = fixture.bootstrap(&["should-not-run"], None, true);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("CI runtime checksum mismatch"));
    assert!(!fixture.runtime_log.exists());
    assert!(!archive_path.exists());
    assert!(
        !fixture
            .cache_directory()
            .join(format!("{}.sha256", fixture.archive_name()))
            .exists()
    );
    assert!(!fixture.curl_log.exists());
}

#[test]
fn explicit_binary_override_runs_without_curl_on_path() {
    let fixture = RuntimeFixture::new();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_ci"));
    let result = fixture.bootstrap(&["--help"], Some(&binary), false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Reusable CI operations"));
    assert!(!fixture.curl_log.exists());
}

fn runtime_platform() -> (&'static str, &'static str, &'static str) {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => ("Linux", "X64", "x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => ("Linux", "ARM64", "aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => ("macOS", "X64", "x86_64-apple-darwin"),
        ("macos", "aarch64") => ("macOS", "ARM64", "aarch64-apple-darwin"),
        ("windows", "x86_64") => ("Windows", "X64", "x86_64-pc-windows-msvc"),
        (platform, architecture) => panic!("unsupported test host {platform}/{architecture}"),
    }
}

fn compile_fixture(source: &Path, executable: &Path) -> PathBuf {
    let status = Command::new("rustc")
        .args(["--edition=2024"])
        .arg(source)
        .arg("-o")
        .arg(executable)
        .status()
        .unwrap();
    assert!(status.success(), "failed to compile {}", source.display());
    executable.to_path_buf()
}

fn copy_fixture_tool(source: &Path, directory: &TempDir, name: &str) -> PathBuf {
    let destination = directory.path().join(executable_name(name));
    fs::copy(source, &destination).unwrap();
    destination
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn locate_program(name: &str) -> PathBuf {
    let program = env::split_paths(&env::var_os("PATH").unwrap())
        .map(|directory| directory.join(executable_name(name)))
        .find(|candidate| candidate.is_file())
        .unwrap();
    let result = Command::new(&program).arg("--version").output().unwrap();
    assert!(result.status.success());
    program
}

#[test]
fn semantic_version_preparation_keeps_manifest_lock_and_loader_in_sync() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = \"personal-ci\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(directory.path().join("Cargo.lock"), "version = 4\n\n[[package]]\nname = \"personal-ci\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dependency\"\nversion = \"0.1.0\"\n").unwrap();
    fs::write(directory.path().join("runtime-version"), "0.1.0\n").unwrap();
    for invalid in ["v0.2.0", "0.2.0-rc.1", "0.2.0+build", "invalid"] {
        assert!(packaging::prepare_version(directory.path(), invalid).is_err());
    }

    assert_eq!(
        fs::read_to_string(directory.path().join("runtime-version")).unwrap(),
        "0.1.0\n"
    );
    packaging::prepare_version(directory.path(), "0.2.0").unwrap();
    packaging::prepare_version(directory.path(), "0.2.0").unwrap();
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(directory.path().join("Cargo.toml")).unwrap()).unwrap();
    assert_eq!(manifest["package"]["version"].as_str(), Some("0.2.0"));
    let lock = fs::read_to_string(directory.path().join("Cargo.lock")).unwrap();
    assert!(lock.contains("name = \"personal-ci\"\nversion = \"0.2.0\""));
    assert!(lock.contains("name = \"dependency\"\nversion = \"0.1.0\""));
    assert_eq!(
        fs::read_to_string(directory.path().join("runtime-version")).unwrap(),
        "0.2.0\n"
    );
}

#[test]
fn package_rejects_a_mismatched_plain_v_tag() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ci"))
        .args(["package", "x86_64-unknown-linux-musl", "missing-binary"])
        .arg(directory.path().join("dist"))
        .env("GITHUB_REF", "refs/tags/v99.0.0")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("release tag must match Cargo package version")
    );
    assert!(!directory.path().join("dist").exists());
}
