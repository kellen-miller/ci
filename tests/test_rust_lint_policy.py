import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PRESET = ROOT / "config/rust"
ACTION = ROOT / ".github/actions/rust-check/check.sh"
PACKAGE = '[package]\nname = "lint_fixture"\nversion = "0.1.0"\nedition = "2024"\n'
VALID_SOURCE = """/// Parse a count.
///
/// # Errors
/// Returns an integer parsing error for invalid input.
pub fn parse_count(input: &str) -> Result<u32, std::num::ParseIntError> {
    input.parse()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_unwrap_and_expect_are_allowed() {
        assert_eq!(super::parse_count("42").unwrap(), 42);
        assert_eq!(super::parse_count("42").expect("valid fixture"), 42);
    }
}
"""
INVALID_SOURCE = """pub fn discarded_result(input: &str) {
    let _ = input.parse::<u32>();
}

pub fn converted_error(input: &str) {
    input.parse::<u32>().ok();
}

pub fn unchecked(input: Option<u32>) -> u32 {
    input.unwrap()
}

pub fn expected(input: Option<u32>) -> u32 {
    input.expect("runtime value")
}

pub fn debugging(value: u32) -> u32 {
    dbg!(value)
}

pub fn unfinished() {
    todo!()
}
"""


class RustLintPolicyTests(unittest.TestCase):
    def test_policy_works_with_shared_action_and_workspace_inheritance(self):
        for workspace in (False, True):
            with self.subTest(workspace=workspace), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                project = root / "project"
                package = project / "app" if workspace else project
                (package / "src").mkdir(parents=True)
                policy = (PRESET / "Cargo.lints.toml").read_text()
                if workspace:
                    policy = policy.replace("[lints.", "[workspace.lints.")
                    (project / "Cargo.toml").write_text(
                        '[workspace]\nmembers = ["app"]\nresolver = "3"\n\n' + policy
                    )
                    (package / "Cargo.toml").write_text(PACKAGE + "\n[lints]\nworkspace = true\n")
                else:
                    (package / "Cargo.toml").write_text(PACKAGE + "\n" + policy)

                for name in ("clippy.toml", "rustfmt.toml"):
                    shutil.copyfile(PRESET / name, project / name)

                source = package / "src/lib.rs"
                source.write_text(VALID_SOURCE)
                environment = {
                    **os.environ,
                    "CARGO_HOME": str(root / "cargo-home"),
                    "CARGO_TARGET_DIR": str(root / "target"),
                    "CARGO_NET_OFFLINE": "true",
                    "RUSTUP_AUTO_INSTALL": "0",
                    "SETUP_TOOLCHAIN": "false",
                    "WORKING_DIRECTORY": str(project),
                    "CHECKS": "fmt clippy test build",
                    "ALL_FEATURES": "true",
                    "BUILD_PROFILE": "dev",
                }
                subprocess.run(
                    ["cargo", "generate-lockfile", "--offline"],
                    cwd=project,
                    env=environment,
                    check=True,
                    capture_output=True,
                )
                result = subprocess.run(
                    ["bash", str(ACTION)],
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("test_unwrap_and_expect_are_allowed ... ok", result.stdout)
                self.assertEqual(source.read_text(), VALID_SOURCE)

                source.write_text(INVALID_SOURCE)
                result = subprocess.run(
                    [
                        "cargo",
                        "clippy",
                        "--locked",
                        "--workspace",
                        "--all-features",
                        "--all-targets",
                        "--message-format=json",
                        "--",
                        "-D",
                        "warnings",
                    ],
                    cwd=project,
                    env=environment,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                diagnostics = [json.loads(line) for line in result.stdout.splitlines()]
                codes = {
                    item["message"]["code"]["code"]
                    for item in diagnostics
                    if item["reason"] == "compiler-message" and item["message"].get("code")
                }
                self.assertLessEqual(
                    {
                        "clippy::unwrap_used",
                        "clippy::expect_used",
                        "clippy::let_underscore_must_use",
                        "clippy::unused_result_ok",
                        "clippy::dbg_macro",
                        "clippy::todo",
                    },
                    codes,
                )
