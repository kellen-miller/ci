#[cfg(test)]
#[path = "fixtures/rust_workspace.rs"]
mod rust_workspace;

use std::fs;
use std::process::Command;

use serde_json::json;

use rust_workspace::{Fixture, diagnostic, names, successful};

fn lines(count: usize) -> String {
    "const _: u8 = 0;\n".repeat(count)
}

#[test]
fn source_limit_counts_handwritten_fixtures_and_excludes_owned_generated_roots() {
    let mut fixture = Fixture::new();
    for name in [
        "handwritten",
        "generated",
        "neighbor",
        "handwritten/dependencies/third_party",
        "handwritten/fixture-package",
    ] {
        fixture.source(&format!("{name}/Cargo.toml"), "[package]\n");
    }

    fixture.source(
        "handwritten/tests/fixtures/fixture.rs",
        "// café\u{2028}same physical line\u{85}still same line\n\n".repeat(375) + &lines(750),
    );
    fixture.source("handwritten/build.rs", lines(749) + "fn final_line() {}");
    fixture.source("handwritten/fixture-package/src/lib.rs", lines(750));
    fixture.source("generated/src/generated.rs", lines(751));
    fixture.source(
        "handwritten/custom build output/generated/ignored.rs",
        lines(751),
    );
    fixture.source(
        "handwritten/dependencies/third_party/src/ignored.rs",
        lines(751),
    );
    fixture.source("handwritten/dependencies/handwritten.rs", lines(750));
    fixture.source("neighbor/src/lib.rs", lines(750));
    fixture.metadata = json!({"workspace_members":["handwritten", "generated", "neighbor"], "target_directory":fixture.project.join("handwritten/custom build output"), "packages":[
        {"id":"handwritten", "name":"handwritten", "manifest_path":fixture.project.join("handwritten/Cargo.toml")},
        {"id":"generated", "name":"generated", "manifest_path":fixture.project.join("generated/Cargo.toml"), "metadata":{"ci":{"generated":true}}},
        {"id":"neighbor", "name":"neighbor", "manifest_path":fixture.project.join("neighbor/Cargo.toml")},
        {"id":"dependency", "manifest_path":fixture.project.join("handwritten/dependencies/third_party/Cargo.toml")}
    ]});
    let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
    successful(&output);
    assert_eq!(names(&calls), ["metadata", "clippy"]);
    assert_eq!(calls[1].cwd, fixture.project);
    assert!(
        calls[1]
            .clippy_conf_dir
            .as_ref()
            .unwrap()
            .ends_with("config/rust")
    );
    for path in [
        "handwritten/tests/fixtures/fixture.rs",
        "handwritten/fixture-package/src/lib.rs",
        "handwritten/dependencies/handwritten.rs",
    ] {
        fixture.source(path, lines(750) + "fn final_line() {}");
        let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
        assert!(!output.status.success());
        diagnostic(&output, path);
        diagnostic(&output, "(751 lines)");
        assert_eq!(names(&calls), ["metadata"]);
        fixture.source(path, lines(750));
    }

    fixture.source(
        "handwritten/tests/fixtures/fixture.rs",
        b"// invalid UTF-8: \xff\n",
    );
    let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
    assert!(!output.status.success());
    diagnostic(&output, "Cannot read Rust source for file-length check");
    assert_eq!(names(&calls), ["metadata"]);
}

#[test]
fn ancestor_dependency_does_not_hide_nested_workspace() {
    let mut fixture = Fixture::new();
    fixture.source("fuzz/Cargo.toml", "[package]\n");
    fixture.source("fuzz/src/lib.rs", lines(751));
    fixture.metadata = json!({"workspace_members":["fuzz"], "target_directory":fixture.temporary.path().join("target"), "packages":[
        {"id":"parent", "manifest_path":fixture.project.join("Cargo.toml")},
        {"id":"fuzz", "name":"fuzz", "manifest_path":fixture.project.join("fuzz/Cargo.toml")}
    ]});
    let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
    assert!(!output.status.success());
    diagnostic(&output, "fuzz/src/lib.rs");
    assert_eq!(names(&calls), ["metadata"]);
}

#[test]
fn git_discovery_keeps_tracked_ignored_sources_and_skips_untracked_ignored_files() {
    let fixture = Fixture::new();
    fixture.source(".gitignore", ".agent/work/\ntracked.rs\n");
    fixture.source("tracked.rs", lines(750));
    fixture.source(".agent/work/archive.rs", lines(751));
    successful(
        &Command::new("git")
            .args(["init", "-q"])
            .arg(&fixture.project)
            .output()
            .unwrap(),
    );
    successful(
        &Command::new("git")
            .arg("-C")
            .arg(&fixture.project)
            .args(["add", "-f", "tracked.rs"])
            .output()
            .unwrap(),
    );
    let (output, _) = fixture.run(&[("CHECKS", "clippy")]);
    successful(&output);
    fixture.source("tracked.rs", lines(751));
    let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
    assert!(!output.status.success());
    diagnostic(&output, "tracked.rs");
    assert_eq!(names(&calls), ["metadata"]);
    fs::remove_file(fixture.project.join("tracked.rs")).unwrap();
    let (output, _) = fixture.run(&[("CHECKS", "clippy")]);
    successful(&output);
}

#[cfg(unix)]
#[test]
fn symlink_ownership_handles_targets_neighbors_and_external_sources() {
    use std::os::unix::fs::symlink;

    let mut fixture = Fixture::new();
    fixture.source("src/lib.rs", lines(750));
    fixture.source("neighbor/Cargo.toml", "[package]\n");
    fixture.source("neighbor/src/lib.rs", lines(750));
    fixture.source("generated/Cargo.toml", "[package]\n");
    fixture.source("generated/src/lib.rs", lines(751));
    fixture.source("dependency/Cargo.toml", "[package]\n");
    fixture.source("dependency/src/lib.rs", lines(751));
    fixture.source("target-output/generated.rs", lines(751));
    fixture.metadata["target_directory"] = json!(fixture.project.join("target-output"));
    fixture.metadata["workspace_members"] = json!(["fixture", "neighbor", "generated"]);
    fixture.metadata["packages"].as_array_mut().unwrap().extend([
        json!({"id":"neighbor", "manifest_path":fixture.project.join("neighbor/Cargo.toml"), "name":"neighbor"}),
        json!({"id":"generated", "manifest_path":fixture.project.join("generated/Cargo.toml"), "name":"generated", "metadata":{"ci":{"generated":true}}}),
        json!({"id":"dependency", "manifest_path":fixture.project.join("dependency/Cargo.toml")})
    ]);
    symlink(
        fixture.project.join("target-output"),
        fixture.project.join("target"),
    )
    .unwrap();
    symlink(
        fixture.project.join("neighbor/src/lib.rs"),
        fixture.project.join("src/neighbor.rs"),
    )
    .unwrap();
    symlink(
        fixture.project.join("src/lib.rs"),
        fixture.project.join("src/internal.rs"),
    )
    .unwrap();
    symlink(
        fixture.project.join("neighbor"),
        fixture.project.join("linked-neighbor"),
    )
    .unwrap();
    let external = fixture.temporary.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("README.md"), "documentation").unwrap();
    symlink(&external, fixture.project.join("assets")).unwrap();
    let (output, _) = fixture.run(&[("CHECKS", "clippy")]);
    successful(&output);
    let link = fixture.project.join("src/bad.rs");
    for (target, message) in [
        (
            fixture.project.join("dependency/src/lib.rs"),
            "points into a dependency package",
        ),
        (
            fixture.project.join("generated/src/lib.rs"),
            "points into a generated workspace package",
        ),
        (
            external.join("README.md"),
            "escapes workspace package ownership",
        ),
        (
            external.join("missing.rs"),
            "Cannot resolve Rust source symlink",
        ),
    ] {
        symlink(target, &link).unwrap();
        let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
        assert!(!output.status.success());
        diagnostic(&output, message);
        assert_eq!(names(&calls), ["metadata"]);
        fs::remove_file(&link).unwrap();
    }

    fs::write(external.join("source.rs"), "// external Rust source").unwrap();
    let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
    assert!(!output.status.success());
    diagnostic(
        &output,
        "directory symlink targets Rust files outside package ownership",
    );
    assert_eq!(names(&calls), ["metadata"]);
}

#[test]
fn encoding_and_comment_rules_are_enforced_at_the_source_boundary() {
    let fixture = Fixture::new();
    for (prefix, existing) in [
        ("\u{feff}// BOM and comment\n", 0),
        (
            "// excluded\r\nconst RAW: &str = r#\"first\r\n// literal\r\n\"#;\r\nconst NORMAL: &str = \"continued\\\r\n\";\r\n",
            5,
        ),
        ("// bare CR stays inside comment\rnot_code();\n", 0),
    ] {
        let source = prefix.to_string() + &"const _: u8 = 0;\r\n".repeat(750 - existing);
        fixture.source("src/lib.rs", &source);
        let (output, _) = fixture.run(&[("CHECKS", "clippy")]);
        successful(&output);
        fixture.source("src/lib.rs", source + "const _: u8 = 1;");
        let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
        assert!(!output.status.success());
        diagnostic(&output, "(751 lines)");
        assert_eq!(names(&calls), ["metadata"]);
    }

    fixture.source(
        "src/lib.rs",
        lines(750) + &"// café\n\n/// docs\n//! docs\n".repeat(500),
    );
    let (output, _) = fixture.run(&[("CHECKS", "clippy")]);
    successful(&output);
}
