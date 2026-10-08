import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = (
    Path(__file__).resolve().parents[1] / ".github/actions/validate-workflow-security/validate.py"
)
SHA = "a" * 40


class WorkflowSecurityTests(unittest.TestCase):
    def scan(self, text, *args):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflow = root / ".github/workflows/test.yaml"
            workflow.parent.mkdir(parents=True)
            workflow.write_text(text)
            return subprocess.run(
                [sys.executable, str(SCRIPT), "--root", directory, *args],
                capture_output=True,
                text=True,
                check=False,
            )

    def test_pinned_checkout_accepts_false_and_ignores_comments(self):
        result = self.scan(
            f"""on: pull_request
# pull_request_target and secrets: inherit are just documentation
jobs:
  check:
    steps:
      - uses: actions/checkout@{SHA}
        with:
          persist-credentials: false
"""
        )
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_pinning_has_no_organization_exemption(self):
        result = self.scan(
            "jobs:\n  check:\n    uses: example/ci/.github/workflows/lint.yaml@main\n"
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("full commit SHA", result.stdout)

    def test_multiline_inheritance_and_target_require_opt_in(self):
        text = f"""on:
  pull_request_target:
jobs:
  check:
    uses: example/ci/.github/workflows/lint.yaml@{SHA}
    secrets:
      inherit
"""
        denied = self.scan(text)
        allowed = self.scan(text, "--allow-pull-request-target", "--allow-secret-inheritance")
        self.assertEqual(denied.returncode, 1)
        self.assertIn("named secrets", denied.stdout)
        self.assertEqual(allowed.returncode, 0, allowed.stdout)

    def test_named_checkout_must_not_persist_credentials(self):
        result = self.scan(
            "jobs:\n  check:\n    steps:\n      - name: Checkout\n"
            f"        uses: actions/checkout@{SHA}\n"
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("persist-credentials", result.stdout)

    def test_docker_action_requires_digest(self):
        for reference, status in [("alpine:latest", 1), (f"alpine@sha256:{'a' * 64}", 0)]:
            with self.subTest(reference=reference):
                result = self.scan(f"runs:\n  steps:\n    - uses: docker://{reference}\n")
                self.assertEqual(result.returncode, status, result.stdout)

    def test_invalid_yaml_fails(self):
        result = self.scan("jobs: [unterminated\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("invalid YAML", result.stdout)

    def test_installed_dependencies_are_not_repository_workflows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflow = root / ".github/workflows/test.yaml"
            workflow.parent.mkdir(parents=True)
            workflow.write_text("on: pull_request\njobs: {}\n")
            dependency = root / ".github/actions/release/node_modules/example/test.yaml"
            dependency.parent.mkdir(parents=True)
            dependency.write_text("invalid: [yaml\n")
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--root", directory],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout)
            self.assertIn("(1 files)", result.stdout)
