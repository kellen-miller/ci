import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ACTION = Path(__file__).resolve().parents[1] / ".github/actions/go-lint"


class GoLintTests(unittest.TestCase):
    def test_real_formatter_discovers_parent_config_and_fails_without_rewriting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "backend"
            project.mkdir()
            config = root / ".golangci.yaml"
            config.write_text('version: "2"\nformatters:\n  enable:\n    - gofmt\n')
            (project / "go.mod").write_text("module example.com/fixture\n\ngo 1.24\n")
            source = project / "main.go"
            source.write_text('package main\nfunc main(){println("fixture")}\n')
            original = source.read_bytes()
            result = subprocess.run(
                ["bash", str(ACTION / "format.sh")],
                cwd=project,
                env=dict(os.environ, LINT_CONFIG=""),
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("main.go", result.stdout)
            self.assertEqual(source.read_bytes(), original)
            subprocess.run(["gofmt", "-w", "main.go"], cwd=project, check=True)
            result = subprocess.run(
                ["bash", str(ACTION / "format.sh")],
                cwd=project,
                env=dict(os.environ, LINT_CONFIG="../.golangci.yaml"),
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(
                config.read_text(), 'version: "2"\nformatters:\n  enable:\n    - gofmt\n'
            )
