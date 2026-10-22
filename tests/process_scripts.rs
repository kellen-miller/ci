#![cfg(test)]

use personal_ci::command;
use std::fs;

#[test]
fn script_launch_preserves_literal_arguments_spaces_and_failure_status() {
    let temporary = tempfile::tempdir().unwrap();
    let script = temporary.path().join("tool with spaces");
    // A shell entry point is the external interface being qualified here.
    fs::write(
        &script,
        "#!/usr/bin/env bash\nif [[ $1 == --fail ]]; then exit 33; fi\nprintf '%s\\n' \"$@\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let marker = temporary.path().join("must-not-exist");
    let argument = format!("$(touch {}) ; \"quoted value\" & %PATH%", marker.display());
    let output = command::capture(command::script(&script).arg(&argument)).unwrap();
    assert_eq!(output, format!("{argument}\n"));
    assert!(!marker.exists());
    let error = command::run(command::script(&script).arg("--fail")).unwrap_err();
    assert_eq!(error.downcast_ref::<command::Failure>().unwrap().0, 33);
}
