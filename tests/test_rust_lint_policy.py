import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
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
    def test_shared_policy_enforces_package_and_workspace_without_manifest_lints(self):
        for workspace in (False, True):
            with self.subTest(workspace=workspace), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                project = root / "project"
                package = project / "app" if workspace else project
                (package / "src").mkdir(parents=True)
                if workspace:
                    (project / "Cargo.toml").write_text(
                        '[workspace]\nmembers = ["app"]\nresolver = "3"\n'
                    )

                (package / "Cargo.toml").write_text(PACKAGE)
                # Conflicting project configs must not override the bundled policy.
                (project / "clippy.toml").write_text("allow-unwrap-in-tests = false\n")
                (project / "rustfmt.toml").write_text("max_width = 20\n")
                preserved = {path: path.read_bytes() for path in project.rglob("*.toml")}
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
                for path, contents in preserved.items():
                    self.assertEqual(path.read_bytes(), contents)

                source.write_text(
                    INVALID_SOURCE
                    + "\npub fn long_lifecycle() {\n"
                    + "    std::hint::black_box(0_u8);\n" * 82
                    + "}\n"
                )
                result = subprocess.run(
                    ["bash", str(ACTION)],
                    env={**environment, "CHECKS": "clippy"},
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                for lint in (
                    "unwrap_used",
                    "expect_used",
                    "let_underscore_must_use",
                    "unused_result_ok",
                    "dbg_macro",
                    "todo",
                    "too_many_lines",
                ):
                    self.assertIn(lint, result.stderr)

                for path, contents in preserved.items():
                    self.assertEqual(path.read_bytes(), contents)
