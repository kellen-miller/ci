#![cfg(test)]

use personal_ci::command;
use std::fs;

#[test]
fn script_launch_preserves_literal_arguments_spaces_and_failure_status() {
    let temporary = tempfile::tempdir().unwrap();
    // Keep the executed inode immutable while parallel tests spawn child processes.
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tool with spaces.sh");

    let marker = temporary.path().join("must-not-exist");
    let argument = format!("$(touch {}) ; \"quoted value\" & %PATH%", marker.display());
    let output = command::capture(command::script(&script).arg(&argument)).unwrap();
    assert_eq!(output, format!("{argument}\n"));
    assert!(!marker.exists());
    let error = command::run(command::script(&script).arg("--fail")).unwrap_err();
    assert_eq!(error.downcast_ref::<command::Failure>().unwrap().0, 33);
}

#[test]
fn npm_shim_preserves_path_like_arguments_through_native_node() {
    let temporary = tempfile::tempdir().unwrap();
    fs::write(
        temporary.path().join("package.json"),
        r#"{"private":true,"scripts":{"arguments":"node arguments.cjs"}}"#,
    )
    .unwrap();
    // Node's process API is the external interface whose arguments are checked.
    fs::write(
        temporary.path().join("arguments.cjs"),
        "process.stdout.write(JSON.stringify(process.argv.slice(2)))",
    )
    .unwrap();
    let arguments = [
        "/literal/path",
        "-Pfoo=/x",
        "$(touch marker) ; quoted & %PATH%",
        "space value",
    ];
    let output = command::capture(
        command::script("npm")
            .args(["--silent", "run", "arguments", "--"])
            .args(arguments)
            .current_dir(temporary.path()),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&output).unwrap(),
        arguments
    );
    assert!(!temporary.path().join("marker").exists());
}
