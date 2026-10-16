import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = yaml.safe_load((ROOT / ".github/workflows/renovate-config-validator.yaml").read_text())
SCRIPT = next(s["run"] for s in WORKFLOW["jobs"]["validate"]["steps"] if s.get("id") == "changes")


class RenovateChangesTests(unittest.TestCase):
    def test_changes(self):
        cases = [
            ("README.md", "renovate.json", "edit", False),
            ("renovate.json", "renovate.json", "edit", True),
            ("config/custom.json5", "config/custom.json5", "edit", True),
            ("default.json", "renovate.json", "edit", True),
            (".github/workflows/shared-ci.yaml", "renovate.json", "edit", True),
            ("renovate.json", "renovate.json", "delete", True),
            ("renovate.json", "renovate.json", "rename", True),
            ("renovate.json", "renovate.json", "base-only", False),
        ]
        for changed, config, operation, expected in cases:
            with (
                self.subTest(changed=changed, operation=operation),
                tempfile.TemporaryDirectory() as d,
            ):
                root = Path(d)
                git = ["git", "-C", d]
                subprocess.run([*git, "init", "-q"], check=True)
                subprocess.run([*git, "config", "user.email", "test@example.com"], check=True)
                subprocess.run([*git, "config", "user.name", "Test"], check=True)
                path = root / changed
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("base")
                subprocess.run([*git, "add", "."], check=True)
                subprocess.run([*git, "commit", "-qm", "base"], check=True)
                base = subprocess.check_output([*git, "rev-parse", "HEAD"], text=True).strip()
                if operation == "delete":
                    path.unlink()
                elif operation == "rename":
                    path.rename(root / "old.json")
                else:
                    path.write_text("changed")

                subprocess.run([*git, "add", "-A"], check=True)
                subprocess.run([*git, "commit", "-qm", "change"], check=True)
                head = subprocess.check_output([*git, "rev-parse", "HEAD"], text=True).strip()
                if operation == "base-only":
                    base, head = head, base

                self.assertEqual(self.run_gate(root, base, head, config), expected)

    def test_non_pr_always_validates(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertTrue(self.run_gate(Path(d), "", "", "renovate.json", "push"))

    def run_gate(self, root, base, head, config, event="pull_request"):
        output = root / "outputs"
        subprocess.run(
            ["bash", "-c", SCRIPT],
            cwd=root,
            check=True,
            env={
                **os.environ,
                "EVENT_NAME": event,
                "BASE_SHA": base,
                "HEAD_SHA": head,
                "CONFIG_FILE": config,
                "GITHUB_OUTPUT": str(output),
                "GITHUB_STEP_SUMMARY": str(root / "summary"),
                "RUNNER_TEMP": str(root),
            },
        )
        return output.read_text().strip() == "validate=true"
