import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = {
    name: yaml.load(
        (ROOT / f".github/workflows/rust-{name}.yaml").read_text(), Loader=yaml.BaseLoader
    )
    for name in ("lint", "test")
}


class RustWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.workspace = Path(self.directory.name) / "nested"
        source = self.workspace / "src"
        source.mkdir(parents=True)
        (self.workspace / "Cargo.toml").write_text(
            '[package]\nname = "ci-fixture"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n'
        )
        self.source = source / "lib.rs"
        self.source.write_text(
            "pub fn value() -> u32 {\n    42\n}\n\n"
            "#[test]\nfn value_is_correct() {\n    assert_eq!(value(), 42);\n}\n"
        )
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline"],
            cwd=self.workspace,
            check=True,
            capture_output=True,
        )

    def run_step(self, workflow, name, **environment):
        step = next(
            step
            for step in WORKFLOWS[workflow]["jobs"][workflow]["steps"]
            if step.get("name") == name
        )
        return subprocess.run(
            ["bash", "-e", "-o", "pipefail", "-c", step["run"]],
            cwd=self.workspace,
            env={**os.environ, **environment},
            capture_output=True,
            text=True,
            check=False,
        )

    def test_nested_workspace_lint_tests_and_build_profiles(self):
        for workflow, name in (
            ("lint", "Check Rust formatting"),
            ("lint", "Run Clippy"),
            ("test", "Run Rust tests"),
        ):
            with self.subTest(step=name):
                result = self.run_step(workflow, name)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

        for release, profile in (("false", "debug"), ("true", "release")):
            with self.subTest(profile=profile):
                result = self.run_step("test", "Build Rust workspace", BUILD_RELEASE=release)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertTrue(
                    (self.workspace / "target" / profile / "libci_fixture.rlib").is_file()
                )

    def test_formatting_and_clippy_fail_without_rewriting(self):
        self.source.write_text("pub fn value()->u32{42}\n")
        original = self.source.read_bytes()
        result = self.run_step("lint", "Check Rust formatting")
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.source.read_bytes(), original)

        self.source.write_text("pub fn value() -> u32 {\n    let answer = 42;\n    answer\n}\n")
        result = self.run_step("lint", "Run Clippy")
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("let_and_return", result.stderr)

    def test_test_failures_propagate(self):
        self.source.write_text("#[test]\nfn fails() {\n    assert_eq!(1, 2);\n}\n")
        result = self.run_step("test", "Run Rust tests")
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("test result: FAILED", result.stdout)

    def test_toolchain_cache_and_optional_build_contract(self):
        for name, workflow in WORKFLOWS.items():
            with self.subTest(workflow=name):
                inputs = workflow["on"]["workflow_call"]["inputs"]
                job = workflow["jobs"][name]
                self.assertEqual(inputs["toolchain"]["default"], "")
                self.assertEqual(job["runs-on"], "${{ inputs.runner }}")
                self.assertEqual(
                    job["defaults"]["run"]["working-directory"],
                    "${{ inputs.working-directory }}",
                )
                setup = next(step for step in job["steps"] if step.get("name") == "Set up Rust")
                self.assertEqual(setup["with"]["rust-src-dir"], "${{ inputs.working-directory }}")
                self.assertEqual(setup["with"]["cache"], "true")
                self.assertEqual(setup["with"]["rustflags"], "")

        test = WORKFLOWS["test"]
        self.assertEqual(test["on"]["workflow_call"]["inputs"]["build"]["default"], "false")
        self.assertEqual(test["jobs"]["test"]["steps"][-1]["if"], "inputs.build")
