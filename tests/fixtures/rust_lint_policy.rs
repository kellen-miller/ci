use super::{diagnostic, fs, rust_workspace, successful};

struct OfflineWorkspace {
    temporary: tempfile::TempDir,
    project: std::path::PathBuf,
}

#[cfg(test)]
impl OfflineWorkspace {
    fn new(manifest: &str, source_name: &str, source: &str) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let project = temporary.path().join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("Cargo.toml"), manifest).unwrap();
        fs::write(project.join("src").join(source_name), source).unwrap();
        let workspace = Self { temporary, project };
        successful(&workspace.cargo(&["generate-lockfile", "--offline"]));
        workspace
    }

    fn cargo(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new("cargo")
            .current_dir(&self.project)
            .args(args)
            .env("CARGO_HOME", self.temporary.path().join("cargo-home"))
            .env("CARGO_TARGET_DIR", self.temporary.path().join("target"))
            .env("CARGO_NET_OFFLINE", "true")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .output()
            .unwrap()
    }

    fn run(&self, checks: &str) -> std::process::Output {
        self.command(checks).output().unwrap()
    }

    fn command(&self, checks: &str) -> std::process::Command {
        let mut command = rust_workspace::process(&self.project);
        command
            .env("CHECKS", checks)
            .env("CARGO_HOME", self.temporary.path().join("cargo-home"))
            .env("CARGO_TARGET_DIR", self.temporary.path().join("target"))
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TERM_COLOR", "never")
            .env("RUSTUP_AUTO_INSTALL", "0");
        command
    }
}

#[test]
fn real_offline_library_keeps_formatting_nonmutating_and_runs_all_targets_and_docs() {
    let workspace = OfflineWorkspace::new(
        "[package]\nname='rust_check_fixture'\nversion='0.1.0'\nedition='2021'\n[features]\nextra=[]\n",
        "lib.rs",
        "/// ```\n/// assert_eq!(rust_check_fixture::answer(), 42);\n/// ```\npub fn answer()->u8{42}\n\n#[cfg(all(test, feature = \"extra\"))]\nmod tests {\n    #[test]\n    fn feature_enabled() {\n        assert_eq!(super::answer(), 42);\n    }\n}\n",
    );
    fs::write(workspace.project.join("rustfmt.toml"), "max_width = 90\n").unwrap();
    fs::create_dir(workspace.project.join("examples")).unwrap();
    fs::write(workspace.project.join("examples/example.rs"), "fn main() {\n    println!(\"{}\", rust_check_fixture::answer());\n}\n\n#[test]\nfn example_test() {\n    assert_eq!(rust_check_fixture::answer(), 42);\n}\n").unwrap();
    let source = workspace.project.join("src/lib.rs");
    let original = fs::read(&source).unwrap();
    let preserved: Vec<_> = ["Cargo.toml", "Cargo.lock", "rustfmt.toml"]
        .iter()
        .map(|name| {
            (
                workspace.project.join(name),
                fs::read(workspace.project.join(name)).unwrap(),
            )
        })
        .collect();
    let output = workspace.run("fmt clippy test build");
    assert!(!output.status.success());
    diagnostic(&output, "lib.rs");
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(!workspace.temporary.path().join("target").exists());
    successful(&workspace.cargo(&["fmt", "--all"]));
    let output = workspace.run("fmt clippy test build");
    successful(&output);
    for message in [
        "Checking rust_check_fixture",
        "tests::feature_enabled ... ok",
        "example_test ... ok",
        "Doc-tests rust_check_fixture",
        "1 passed",
    ] {
        diagnostic(&output, message);
    }

    assert!(
        workspace
            .temporary
            .path()
            .join("target/debug/librust_check_fixture.rlib")
            .is_file()
    );
    for (path, content) in preserved {
        assert_eq!(fs::read(path).unwrap(), content);
    }

    successful(
        &workspace
            .command("build")
            .env("BUILD_PROFILE", "release")
            .output()
            .unwrap(),
    );
    assert!(
        workspace
            .temporary
            .path()
            .join("target/release/librust_check_fixture.rlib")
            .is_file()
    );
    fs::write(
        &source,
        "pub fn answer() -> u8 {\n    let answer = 42;\n    answer\n}\n",
    )
    .unwrap();
    let output = workspace.run("clippy");
    assert!(!output.status.success());
    diagnostic(&output, "let_and_return");
    fs::write(
        &source,
        "pub fn answer() -> u8 {42}\n#[test]\nfn fails() { assert_eq!(1,2); }\n",
    )
    .unwrap();
    let output = workspace.run("test");
    assert!(!output.status.success());
    diagnostic(&output, "test result: FAILED");
}

#[test]
fn real_offline_binary_only_package_never_requests_library_doctests() {
    let workspace = OfflineWorkspace::new(
        "[package]\nname='binary_fixture'\nversion='0.1.0'\nedition='2021'\n",
        "main.rs",
        "fn main() {}\n",
    );
    successful(&workspace.run("fmt clippy test build"));
    assert!(
        workspace
            .temporary
            .path()
            .join("target/debug/binary_fixture")
            .is_file()
    );
}

#[test]
fn shared_policy_enforces_package_and_workspace_without_manifest_lints() {
    let valid = "/// Parse a count.\n///\n/// # Errors\n/// Returns a parsing error for invalid input.\npub fn parse_count(input: &str) -> Result<u32, std::num::ParseIntError> {\n    input.parse()\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_unwrap_and_expect_are_allowed() {\n        assert_eq!(super::parse_count(\"42\").unwrap(), 42);\n        assert_eq!(super::parse_count(\"42\").expect(\"valid fixture\"), 42);\n    }\n}\n";
    for workspace_mode in [false, true] {
        let workspace = OfflineWorkspace::new(
            "[package]\nname='lint_fixture'\nversion='0.1.0'\nedition='2024'\n",
            "lib.rs",
            valid,
        );
        let source = if workspace_mode {
            fs::create_dir_all(workspace.project.join("app")).unwrap();
            fs::rename(
                workspace.project.join("src"),
                workspace.project.join("app/src"),
            )
            .unwrap();
            fs::rename(
                workspace.project.join("Cargo.toml"),
                workspace.project.join("app/Cargo.toml"),
            )
            .unwrap();
            fs::write(
                workspace.project.join("Cargo.toml"),
                "[workspace]\nmembers=['app']\nresolver='3'\n",
            )
            .unwrap();
            successful(&workspace.cargo(&["generate-lockfile", "--offline"]));
            workspace.project.join("app/src/lib.rs")
        } else {
            workspace.project.join("src/lib.rs")
        };
        fs::write(
            workspace.project.join("clippy.toml"),
            "allow-unwrap-in-tests = false\n",
        )
        .unwrap();
        fs::write(workspace.project.join("rustfmt.toml"), "max_width = 20\n").unwrap();
        let preserved: Vec<_> = ["Cargo.toml", "Cargo.lock", "clippy.toml", "rustfmt.toml"]
            .iter()
            .map(|name| {
                (
                    workspace.project.join(name),
                    fs::read(workspace.project.join(name)).unwrap(),
                )
            })
            .collect();
        let output = workspace.run("fmt clippy test build");
        successful(&output);
        diagnostic(&output, "test_unwrap_and_expect_are_allowed ... ok");
        assert_eq!(fs::read_to_string(&source).unwrap(), valid);
        let invalid = "pub fn discarded_result(input: &str) { let _ = input.parse::<u32>(); }\npub fn converted_error(input: &str) { input.parse::<u32>().ok(); }\npub fn unchecked(input: Option<u32>) -> u32 { input.unwrap() }\npub fn expected(input: Option<u32>) -> u32 { input.expect(\"runtime value\") }\npub fn debugging(value: u32) -> u32 { dbg!(value) }\npub fn unfinished() { todo!() }\npub fn long_lifecycle() {\n".to_string() + &"    std::hint::black_box(0_u8);\n".repeat(252) + "}\n";
        fs::write(&source, invalid).unwrap();
        let output = workspace.run("clippy");
        assert!(!output.status.success());
        for lint in [
            "unwrap_used",
            "expect_used",
            "let_underscore_must_use",
            "unused_result_ok",
            "dbg_macro",
            "todo",
            "too_many_lines",
        ] {
            diagnostic(&output, lint);
        }

        for (path, content) in preserved {
            assert_eq!(fs::read(path).unwrap(), content);
        }
    }
}

#[test]
fn generated_workspace_retains_format_test_build_and_dependency_compilation() {
    let workspace = OfflineWorkspace::new(
        "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        "lib.rs",
        "pub fn answer() -> u32 {\n    generated::answer()\n}\n",
    );
    fs::create_dir_all(workspace.project.join("app")).unwrap();
    fs::rename(
        workspace.project.join("src"),
        workspace.project.join("app/src"),
    )
    .unwrap();
    fs::rename(
        workspace.project.join("Cargo.toml"),
        workspace.project.join("app/Cargo.toml"),
    )
    .unwrap();
    let app_manifest = workspace.project.join("app/Cargo.toml");
    fs::write(
        &app_manifest,
        fs::read_to_string(&app_manifest).unwrap()
            + "[dependencies]\ngenerated={path='../generated'}\n",
    )
    .unwrap();
    fs::write(
        workspace.project.join("Cargo.toml"),
        "[workspace]\nmembers=['app','generated']\nresolver='3'\n",
    )
    .unwrap();
    fs::create_dir_all(workspace.project.join("generated/src")).unwrap();
    let manifest = "[package]\nname='generated'\nversion='0.1.0'\nedition='2024'\n";
    let marker = "[package.metadata.ci]\ngenerated=true\n";
    let generated_manifest = workspace.project.join("generated/Cargo.toml");
    let generated_source = workspace.project.join("generated/src/lib.rs");
    let app_source = workspace.project.join("app/src/lib.rs");
    let handwritten = fs::read_to_string(&app_source).unwrap();
    let generated = "/// ```\n/// assert_eq!(generated::answer(), 42);\n/// ```\npub fn answer() -> u32 {\n    42\n}\n\npub fn parse_count(input: &str) -> Result<u32, std::num::ParseIntError> {\n    input.parse()\n}\n\npub fn long_lifecycle() {\n".to_string() + &"    std::hint::black_box(0_u8);\n".repeat(252) + "}\n\n#[test]\nfn generated_test() {\n    assert_eq!(answer(), 42);\n}\n";
    fs::write(&generated_manifest, manifest.to_string() + marker).unwrap();
    fs::write(&generated_source, &generated).unwrap();
    successful(&workspace.cargo(&["generate-lockfile", "--offline"]));
    let output = workspace.run("fmt clippy test build");
    successful(&output);
    diagnostic(&output, "generated_test ... ok");
    diagnostic(&output, "Doc-tests generated");
    assert!(
        workspace
            .temporary
            .path()
            .join("target/debug/libgenerated.rlib")
            .is_file()
    );
    assert!(
        workspace
            .temporary
            .path()
            .join("target/debug/libapp.rlib")
            .is_file()
    );
    for (checks, metadata, app, emitted, diagnostic_text) in [
        (
            "clippy",
            "",
            handwritten.as_str(),
            generated.as_str(),
            "missing_errors_doc",
        ),
        (
            "clippy",
            "",
            handwritten.as_str(),
            generated.as_str(),
            "too_many_lines",
        ),
        (
            "clippy",
            marker,
            generated.as_str(),
            generated.as_str(),
            "missing_errors_doc",
        ),
        (
            "fmt",
            marker,
            handwritten.as_str(),
            "pub fn broken(){ }\n",
            "Diff in",
        ),
        (
            "test",
            marker,
            handwritten.as_str(),
            "pub fn broken(\n",
            "error:",
        ),
        (
            "build",
            marker,
            handwritten.as_str(),
            "pub fn broken(\n",
            "error:",
        ),
        (
            "test",
            marker,
            handwritten.as_str(),
            &generated.replace("answer(), 42);", "answer(), 0);"),
            "generated_test ... FAILED",
        ),
    ] {
        fs::write(&generated_manifest, manifest.to_string() + metadata).unwrap();
        fs::write(&app_source, app).unwrap();
        fs::write(&generated_source, emitted).unwrap();
        let output = workspace.run(checks);
        assert!(!output.status.success());
        diagnostic(&output, diagnostic_text);
    }
}

#[test]
fn shared_policy_keeps_cohesive_mapping_limit_and_explicit_private_visibility() {
    let workspace = OfflineWorkspace::new(
        "[package]\nname='wire_fixture'\nversion='0.1.0'\nedition='2021'\n",
        "lib.rs",
        "",
    );
    let source = workspace.project.join("src/lib.rs");
    for (variants, expected) in [(12, true), (32, false)] {
        let names: Vec<_> = (0..variants).map(|index| format!("Frame{index}")).collect();
        let fields: String = (0..6)
            .map(|index| format!("    pub field{index}: u8,\n"))
            .collect();
        let arms: String = names
            .iter()
            .enumerate()
            .map(|(value, name)| {
                format!(
                    "        FrameKind::{name} => WireFields {{\n{}        }},\n",
                    (0..6)
                        .map(|field| format!("            field{field}: {value},\n"))
                        .collect::<String>()
                )
            })
            .collect();
        fs::write(&source, format!("pub enum FrameKind {{ {} }}\npub struct WireFields {{\n{fields}}}\npub fn decode(kind: FrameKind) -> WireFields {{\n    match kind {{\n{arms}    }}\n}}\n", names.join(", "))).unwrap();
        let output = workspace.run("clippy");
        assert_eq!(output.status.success(), expected);
        if !expected {
            diagnostic(&output, "too_many_lines");
        }
    }

    fs::write(&source, "mod implementation {\n    pub(crate) fn answer() -> u8 {42}\n}\npub fn answer() -> u8 { implementation::answer() }\n").unwrap();
    successful(&workspace.run("clippy"));
    fs::write(
        &source,
        fs::read_to_string(&source)
            .unwrap()
            .replace("pub(crate) fn", "pub fn"),
    )
    .unwrap();
    let output = workspace.run("clippy");
    assert!(!output.status.success());
    diagnostic(&output, "unreachable_pub");
}
