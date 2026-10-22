#[cfg(test)]
#[path = "fixtures/rust_workspace.rs"]
mod rust_workspace;

use std::fs;
use std::path::Path;

use personal_ci::rust_lexer::code_lines;
use serde_json::{Value, json};

use rust_workspace::{Fixture, ROOT, diagnostic, names, successful};

#[test]
fn default_checks_lock_workspace_and_override_ambient_policy() {
    let fixture = Fixture::new();
    let (output, calls) = fixture.run(&[("CLIPPY_CONF_DIR", "ambient")]);
    successful(&output);
    assert_eq!(
        calls[0].argv,
        [
            "fmt",
            "--all",
            "--",
            "--check",
            "--config-path",
            &format!("{ROOT}/config/rust/rustfmt.toml")
        ]
    );
    assert_eq!(
        &calls[2].argv[..6],
        [
            "clippy",
            "--locked",
            "--workspace",
            "--all-features",
            "--all-targets",
            "--"
        ]
    );
    let flags = &calls[2].argv;
    assert_eq!(&flags[flags.len() - 2..], ["-D", "warnings"]);
    assert!(flags.contains(&"unused_must_use".into()));
    assert!(
        flags.iter().position(|flag| flag == "clippy::all").unwrap()
            < flags
                .iter()
                .position(|flag| flag == "clippy::unwrap_used")
                .unwrap()
    );
    assert_eq!(
        calls[2].clippy_conf_dir,
        Some(Path::new(ROOT).join("config/rust"))
    );
    assert_eq!(
        calls[1].argv,
        [
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--all-features"
        ]
    );
    assert_eq!(
        calls[3].argv,
        [
            "test",
            "--locked",
            "--workspace",
            "--all-features",
            "--all-targets"
        ]
    );
    assert_eq!(
        calls[4].argv,
        [
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--all-features"
        ]
    );
    assert_eq!(
        calls[5].argv,
        ["test", "--locked", "--workspace", "--all-features", "--doc"]
    );
    assert_eq!(
        calls[6].argv,
        [
            "build",
            "--locked",
            "--workspace",
            "--all-features",
            "--all-targets",
            "--profile",
            "dev"
        ]
    );
    assert!(calls.iter().all(|call| call.cwd == fixture.project));
}

#[test]
fn ordered_selected_checks_use_release_and_default_features() {
    let fixture = Fixture::new();
    let (output, calls) = fixture.run(&[
        ("CHECKS", " build   test clippy "),
        ("ALL_FEATURES", "false"),
        ("BUILD_PROFILE", "release"),
    ]);
    successful(&output);
    assert_eq!(
        names(&calls),
        ["build", "test", "metadata", "test", "metadata", "clippy"]
    );
    assert_eq!(
        calls[0].argv,
        [
            "build",
            "--locked",
            "--workspace",
            "--all-targets",
            "--profile",
            "release"
        ]
    );
    assert!(
        calls
            .iter()
            .all(|call| !call.argv.iter().any(|arg| arg == "--all-features"))
    );
    for (check, expected) in [
        ("fmt", vec!["fmt"]),
        ("clippy", vec!["metadata", "clippy"]),
        ("test", vec!["test", "metadata", "test"]),
        ("build", vec!["build"]),
    ] {
        let (output, calls) = fixture.run(&[("CHECKS", check)]);
        successful(&output);
        assert_eq!(names(&calls), expected);
    }
}

#[test]
fn custom_policy_is_workspace_relative_and_prevalidated() {
    let fixture = Fixture::new();
    let policy = fixture.project.join("local policy");
    fs::create_dir(&policy).unwrap();
    let (output, calls) = fixture.run(&[("POLICY_DIRECTORY", "local policy")]);
    assert!(!output.status.success());
    assert!(calls.is_empty());
    fs::write(policy.join("rustfmt.toml"), "max_width = 88\n").unwrap();
    fs::write(policy.join("clippy.toml"), "").unwrap();
    for content in [
        "invalid toml",
        "[rust]\nunused_must_use = 'warn'\n",
        "[rust]\nx = 'deny'\n[clippy]\ny = 'invalid'\n",
        "[rust]\n'bad-name' = 'deny'\n[clippy]\ny = 'warn'\n",
        "[rust]\n[clippy]\ny = 'warn'\n",
        "[rust]\nx = 'deny'\n[clippy]\ny = 'warn'\n[extra]\nx = 'warn'\n",
    ] {
        fs::write(policy.join("lints.toml"), content).unwrap();
        let (output, calls) = fixture.run(&[("POLICY_DIRECTORY", "local policy")]);
        assert!(!output.status.success());
        assert!(calls.is_empty());
    }

    fs::write(
        policy.join("lints.toml"),
        "[rust]\nunused_must_use = 'deny'\n[clippy]\nunwrap_used = 'warn'\n",
    )
    .unwrap();
    let (output, calls) = fixture.run(&[
        ("POLICY_DIRECTORY", "local policy"),
        ("CHECKS", "fmt clippy"),
    ]);
    successful(&output);
    assert_eq!(
        calls[0].argv.last(),
        Some(&policy.join("rustfmt.toml").display().to_string())
    );
    assert_eq!(
        &calls[2].argv[6..],
        [
            "-D",
            "unused_must_use",
            "-W",
            "clippy::unwrap_used",
            "-D",
            "warnings"
        ]
    );
    assert_eq!(calls[2].clippy_conf_dir, Some(policy));
}

#[test]
fn invalid_inputs_fail_before_commands_and_shell_text_stays_literal() {
    let fixture = Fixture::new();
    let inputs = [
        (
            "CHECKS",
            vec![
                "",
                "   ",
                "fmt unknown",
                "fmt fmt",
                "fmt clippy fmt",
                "fmt\nbuild",
                "fmt\rbuild",
                "fmt\tbuild",
                "fmt\n",
                "fmt;touch pwned",
                "fmt $(touch pwned)",
                "fmt `touch pwned`",
                "fmt *",
                "fmt --release",
            ],
        ),
        (
            "ALL_FEATURES",
            vec!["", "True", "1", "false ", "true\n", "$(touch pwned)"],
        ),
        (
            "SETUP_TOOLCHAIN",
            vec!["", "True", "1", "false ", "true\n", "$(touch pwned)"],
        ),
        (
            "BUILD_PROFILE",
            vec![
                "",
                "debug",
                "custom",
                "dev release",
                "dev\r",
                ";touch pwned",
            ],
        ),
        (
            "WORKING_DIRECTORY",
            vec!["workspace\nname", "workspace\rname", "", "missing"],
        ),
        (
            "POLICY_DIRECTORY",
            vec!["bad\npath", "bad\rpath", "missing"],
        ),
    ];
    for (name, values) in inputs {
        for value in values {
            let (output, calls) = fixture.run(&[(name, value)]);
            assert!(!output.status.success(), "{name}={value:?}");
            assert!(calls.is_empty(), "{name}={value:?}: {calls:?}");
        }
    }

    let directory = fixture
        .temporary
        .path()
        .join("$(touch pwned); `touch pwned` *");
    fs::create_dir(&directory).unwrap();
    let (output, calls) = fixture.run(&[
        ("WORKING_DIRECTORY", directory.to_str().unwrap()),
        ("CHECKS", "fmt"),
    ]);
    successful(&output);
    assert_eq!(calls[0].cwd, directory.canonicalize().unwrap());
    assert!(!fixture.temporary.path().join("pwned").exists());
}

#[test]
fn failures_preserve_child_status_and_stop_later_commands() {
    let fixture = Fixture::new();
    for (check, argument, count) in [
        ("fmt", "", 1),
        ("metadata", "", 2),
        ("clippy", "", 3),
        ("test", "--all-targets", 4),
        ("metadata", "--no-deps", 5),
        ("test", "--doc", 6),
        ("build", "", 7),
    ] {
        let (output, calls) = fixture.run(&[
            ("FAIL_CHECK", check),
            ("FAIL_ARG", argument),
            ("FAIL_CODE", "43"),
        ]);
        assert_eq!(output.status.code(), Some(43));
        assert_eq!(calls.len(), count);
        assert_eq!(calls.last().unwrap().argv[0], check);
    }

    for checks in ["clippy build", "test build"] {
        let (output, calls) =
            fixture.run(&[("CHECKS", checks), ("CARGO_METADATA", "invalid JSON")]);
        assert!(!output.status.success());
        assert_eq!(
            names(&calls),
            if checks.starts_with("clippy") {
                vec!["metadata"]
            } else {
                vec!["test", "metadata"]
            }
        );
    }
}

#[test]
fn generated_packages_require_exact_boolean_and_keep_other_checks() {
    let mut fixture = Fixture::new();
    for name in ["app", "proto", "sql", "dependency"] {
        fixture.source(&format!("{name}/Cargo.toml"), "[package]\n");
    }

    fixture.metadata = json!({"workspace_members":["app", "proto", "sql"], "target_directory":fixture.temporary.path().join("target"), "packages":[
        {"id":"app", "name":"app", "manifest_path":fixture.project.join("app/Cargo.toml")},
        {"id":"proto", "name":"proto", "manifest_path":fixture.project.join("proto/Cargo.toml"), "metadata":{"ci":{"generated":true}}},
        {"id":"sql", "name":"sql", "manifest_path":fixture.project.join("sql/Cargo.toml"), "metadata":{"ci":{"generated":true}}},
        {"id":"dependency", "name":"dependency", "manifest_path":fixture.project.join("dependency/Cargo.toml"), "metadata":{"ci":{"generated":true}}}
    ]});
    for metadata in [
        Value::Null,
        json!({}),
        json!({"ci":true}),
        json!({"ci":{"generated":false}}),
        json!({"ci":{"generated":"true"}}),
        json!({"ci":{"generated":1}}),
    ] {
        fixture.metadata["packages"][0]["metadata"] = metadata;
        let (output, calls) = fixture.run(&[("CHECKS", "clippy")]);
        successful(&output);
        assert_eq!(
            &calls[1].argv[..11],
            [
                "clippy",
                "--locked",
                "--workspace",
                "--all-features",
                "--no-deps",
                "--exclude",
                "proto",
                "--exclude",
                "sql",
                "--all-targets",
                "--"
            ]
        );
    }

    fixture.metadata["packages"][0]["metadata"] = json!({"ci":{"generated":true}});
    fixture.metadata["packages"][0]["targets"] = json!([{"doctest":true}]);
    let (output, calls) = fixture.run(&[]);
    successful(&output);
    diagnostic(&output, "Skipping Clippy: all workspace packages");
    assert_eq!(
        names(&calls),
        ["fmt", "metadata", "test", "metadata", "test", "build"]
    );
    assert!(
        calls
            .iter()
            .all(|call| !call.argv.iter().any(|argument| argument == "--exclude"))
    );
}

#[test]
fn doctests_only_use_workspace_members() {
    let mut fixture = Fixture::new();
    fixture.metadata["packages"][0]["targets"] = json!([{"doctest":false}]);
    fixture.metadata["packages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"dependency", "targets":[{"doctest":true}]}));
    let (output, calls) = fixture.run(&[("CHECKS", "test build")]);
    successful(&output);
    assert_eq!(names(&calls), ["test", "metadata", "build"]);
    assert!(calls[1].argv.iter().any(|argument| argument == "--no-deps"));
}

#[test]
fn lexer_preserves_rust_literal_comment_and_physical_line_rules() {
    let source = "//! café\n/* outer\n /* nested */\n*/\n\n".to_string()
        + &"fn item() {} // comment\n".repeat(738)
        + "/* leading */ fn inline() {} /* middle */ fn after() {}\n"
        + r####"fn markers<'a>(value: &'a str) { let _: &'static str = "// /* */"; let _ = "escaped \" // /* */"; let _ = b"// /* */"; let _ = br##"// /* */"##; let _ = b'/'; let _ = '/'; let _ = value; } /* inline */"####
        + "\nconst RAW: &str = r###\"\n\n   \n// raw string content counts /* here */\n\"###;\nconst ZERO_HASH_RAW: &str = r\"// /*\";\nconst NORMAL: &str = \"first\n   \n// normal string content counts\n\";\nconst ESCAPED: &str = \"continued\\\n\";\n// excluded after escaped newline\nconst VALUE: u8 = 1;\n// café\n\n";
    assert_eq!(code_lines(&source), 750);
    assert_eq!(code_lines(&(source + "fn over_limit() {}")), 751);
    assert_eq!(
        code_lines("let chars = ['\\u{2f}', '\\x2f', '\\'', 'é']; /* comment\n*/\nnext();"),
        2
    );
    assert_eq!(
        code_lines("const C: &core::ffi::CStr = cr##\"\n// literal\n\"##;\n"),
        3
    );
    for (prefix, count) in [
        ("\u{feff}// BOM\n", 0),
        (
            "// excluded\r\nconst RAW: &str = r#\"first\r\n// literal\r\n\"#;\r\nconst NORMAL: &str = \"continued\\\r\n\";\r\n",
            5,
        ),
        ("// bare CR stays in comment\rnot_code();\n", 0),
    ] {
        assert_eq!(
            code_lines(&(prefix.to_string() + &"const _: u8 = 0;\r\n".repeat(750 - count))),
            750
        );
    }
}

#[cfg(test)]
#[path = "fixtures/rust_lint_policy.rs"]
mod lint_policy;
