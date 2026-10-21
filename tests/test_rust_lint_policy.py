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
    def test_generated_packages_skip_clippy_but_retain_workspace_checks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["app", "generated"]\nresolver = "3"\n'
            )
            app = root / "app"
            generated = root / "generated"
            for package in (app, generated):
                (package / "src").mkdir(parents=True)

            (app / "Cargo.toml").write_text(
                '[package]\nname = "app"\nversion = "0.1.0"\nedition = "2024"\n'
                '[dependencies]\ngenerated = { path = "../generated" }\n'
            )
            manifest = '[package]\nname = "generated"\nversion = "0.1.0"\nedition = "2024"\n'
            marker = "\n[package.metadata.ci]\ngenerated = true\n"
            (generated / "Cargo.toml").write_text(manifest + marker)
            app_source = "pub fn answer() -> u32 {\n    generated::answer()\n}\n"
            source = (
                "/// ```\n/// assert_eq!(generated::answer(), 42);\n/// ```\n"
                "pub fn answer() -> u32 {\n    42\n}\n\n"
                "pub fn parse_count(input: &str) -> Result<u32, std::num::ParseIntError> {\n"
                "    input.parse()\n}\n\n"
                "pub fn long_lifecycle() {\n"
                + "    std::hint::black_box(0_u8);\n" * 252
                + "}\n\n#[test]\nfn generated_test() {\n    assert_eq!(answer(), 42);\n}\n"
            )
            (app / "src/lib.rs").write_text(app_source)
            (generated / "src/lib.rs").write_text(source)
            environment = {
                **os.environ,
                "CARGO_HOME": str(root / "cargo-home"),
                "CARGO_TARGET_DIR": str(root / "target"),
                "CARGO_PROFILE_DEV_DEBUG": "0",
                "CARGO_NET_OFFLINE": "true",
                "CARGO_TERM_COLOR": "never",
                "RUSTUP_AUTO_INSTALL": "0",
                "SETUP_TOOLCHAIN": "false",
                "WORKING_DIRECTORY": str(root),
                "CHECKS": "fmt clippy test build",
                "ALL_FEATURES": "true",
                "BUILD_PROFILE": "dev",
            }
            subprocess.run(
                ["cargo", "generate-lockfile", "--offline"],
                cwd=root,
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
            self.assertIn("generated_test ... ok", result.stdout)
            self.assertIn("Doc-tests generated", result.stderr)
            self.assertTrue((root / "target/debug/libgenerated.rlib").is_file())
            self.assertTrue((root / "target/debug/libapp.rlib").is_file())

            for checks, metadata, handwritten, emitted, diagnostic in (
                ("clippy", "", app_source, source, "missing_errors_doc"),
                ("clippy", "", app_source, source, "too_many_lines"),
                ("clippy", marker, source, source, "missing_errors_doc"),
                ("fmt", marker, app_source, "pub fn broken(){ }\n", "Diff in"),
                ("test", marker, app_source, "pub fn broken(\n", "error:"),
                ("build", marker, app_source, "pub fn broken(\n", "error:"),
                (
                    "test",
                    marker,
                    app_source,
                    source.replace("answer(), 42);", "answer(), 0);"),
                    "generated_test ... FAILED",
                ),
            ):
                with self.subTest(checks=checks, diagnostic=diagnostic):
                    (generated / "Cargo.toml").write_text(manifest + metadata)
                    (app / "src/lib.rs").write_text(handwritten)
                    (generated / "src/lib.rs").write_text(emitted)
                    result = subprocess.run(
                        ["bash", str(ACTION)],
                        env={**environment, "CHECKS": checks},
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertIn(diagnostic, result.stdout + result.stderr)

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
                    + "    std::hint::black_box(0_u8);\n" * 252
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
