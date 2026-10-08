import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ACTION = Path(__file__).resolve().parents[1] / ".github/actions/setup-node"


class NodeSetupTests(unittest.TestCase):
    def test_subdirectory_detection_and_node_file_precedence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "frontend"
            project.mkdir()
            (project / "package.json").write_text(json.dumps({"engines": {"node": ">=22"}}))
            (project / "pnpm-lock.yaml").touch()
            (project / ".nvmrc").write_text("24\n")
            output = root / "outputs"
            env = dict(os.environ, INPUT_WD="frontend", GITHUB_OUTPUT=str(output))
            result = subprocess.run(
                [sys.executable, str(ACTION / "detect.py")],
                cwd=project,
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("lockfile=frontend/pnpm-lock.yaml", output.read_text())
            self.assertIn("node-version=24", output.read_text())

    def test_ambiguous_lockfiles_fail_without_explicit_manager(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "package.json").write_text("{}")
            (root / "package-lock.json").touch()
            (root / "yarn.lock").touch()
            env = dict(os.environ, INPUT_WD=".", GITHUB_OUTPUT=str(root / "outputs"))
            result = subprocess.run(
                [sys.executable, str(ACTION / "detect.py")],
                cwd=root,
                env=env,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)

    def test_real_npm_install_is_frozen_and_preserves_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "package.json").write_text('{"name":"fixture","version":"1.0.0"}')
            subprocess.run(
                ["npm", "install", "--package-lock-only", "--ignore-scripts", "--no-audit"],
                cwd=root,
                check=True,
                capture_output=True,
            )
            env = dict(os.environ, PM="npm")
            success = subprocess.run(
                ["bash", str(ACTION / "install.sh")],
                cwd=root,
                env=env,
                capture_output=True,
                check=False,
            )
            self.assertEqual(success.returncode, 0, success.stderr)
            lock = (root / "package-lock.json").read_bytes()
            (root / "package.json").write_text(
                '{"name":"fixture","version":"1.0.0","dependencies":{"missing":"file:absent"}}'
            )
            failure = subprocess.run(
                ["bash", str(ACTION / "install.sh")],
                cwd=root,
                env=env,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(failure.returncode, 0)
            self.assertEqual((root / "package-lock.json").read_bytes(), lock)
