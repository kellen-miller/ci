import json
import os
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path

ACTION = Path(__file__).resolve().parents[1] / ".github/actions/go-lint"
POLICY = ACTION.parents[2] / "config/golangci.yaml"


class GoLintTests(unittest.TestCase):
    def test_wasm_entrypoint_init_exception_does_not_cover_other_files(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            (project / "go.mod").write_text("module example.com/fixture\n\ngo 1.24\n")
            for target in ("wasm", "native"):
                package = project / "cmd" / target
                package.mkdir(parents=True)
                (package / "main.go").write_text(
                    "package main\n\nfunc init() {}\n\nfunc main() {}\n"
                )

            (project / "cmd/wasm/extra.go").write_text("package main\n\nfunc init() {}\n")
            result = subprocess.run(
                [
                    "golangci-lint",
                    "run",
                    "--allow-parallel-runners",
                    "--enable-only",
                    "gochecknoinits",
                    "--config",
                    str(POLICY),
                ],
                cwd=project,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("cmd/native/main.go:", result.stdout)
            self.assertIn("cmd/wasm/extra.go:", result.stdout)
            self.assertNotIn("cmd/wasm/main.go:", result.stdout)

    def test_external_shared_policy_checks_consumer_module_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            (project / "go.mod").write_text("module example.com/fixture\n\ngo 1.24\n")
            (project / ".golangci.yaml").write_text(
                'version: "2"\nlinters:\n  default: none\n  enable: [govet]\n'
            )
            source = project / "record.go"
            source.write_text(
                'package fixture\n\nimport "log"\n\nfunc Record() {\n\tlog.Print("record")\n}\n'
            )
            original = source.read_bytes()
            result = subprocess.run(
                ["golangci-lint", "run", "--config", str(POLICY)],
                cwd=project,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("depguard", result.stdout)
            self.assertIn("record.go", result.stdout)
            self.assertNotIn("../", result.stdout)
            self.assertEqual(source.read_bytes(), original)

    def test_lint_and_formatter_share_resolved_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            project = root / "backend"
            project.mkdir()
            (root / ".golangci.yaml").write_text('version: "2"\n')
            (project / ".golangci.yaml").write_text('version: "2"\n')
            override = root / "policy 'single' \"double\" $(touch injected).yaml"
            override.write_text('version: "2"\n')
            binary = root / "golangci-lint"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, pathlib, sys\n"
                "pathlib.Path(os.environ['CAPTURE']).write_text(json.dumps(sys.argv[1:]))\n"
            )
            binary.chmod(0o755)
            output = root / "output"
            capture = root / "capture"
            for config, expected in [
                ("", POLICY),
                (f"../{override.name}", override),
                (str(override), override),
            ]:
                with self.subTest(config=config):
                    env = dict(
                        os.environ,
                        LINT_CONFIG=config,
                        GITHUB_OUTPUT=str(output),
                        CAPTURE=str(capture),
                        PATH=f"{root}{os.pathsep}{os.environ['PATH']}",
                    )
                    output.unlink(missing_ok=True)
                    lint = subprocess.run(
                        ["python3", str(ACTION / "config.py"), "--github-output"],
                        cwd=project,
                        env=env,
                        capture_output=True,
                        text=True,
                        check=True,
                    )
                    self.assertEqual(lint.stdout.strip(), str(expected))
                    args = shlex.split(output.read_text().removeprefix("args="))
                    self.assertEqual(args, ["--config", str(expected)])
                    formatted = subprocess.run(
                        ["bash", str(ACTION / "format.sh")],
                        cwd=project,
                        env=env,
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertEqual(formatted.returncode, 0, formatted.stderr)
                    self.assertEqual(json.loads(capture.read_text()), ["fmt", "--diff", *args])
                    self.assertFalse((project / "injected").exists())
                    self.assertEqual(override.read_text(), 'version: "2"\n')

    def test_missing_or_multiline_override_fails_for_lint_and_format(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            for config, error in [
                ("missing.yaml", "config file does not exist:"),
                ("bad\nname.yaml", "config must be a single-line path"),
                ("bad\rname.yaml", "config must be a single-line path"),
            ]:
                for command in [
                    ["python3", str(ACTION / "config.py"), "--github-output"],
                    ["bash", str(ACTION / "format.sh")],
                ]:
                    with self.subTest(config=config, command=command):
                        output = root / "output"
                        result = subprocess.run(
                            command,
                            cwd=root,
                            env=dict(os.environ, LINT_CONFIG=config, GITHUB_OUTPUT=str(output)),
                            capture_output=True,
                            text=True,
                            check=False,
                        )
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn(error, result.stderr)
                        self.assertFalse(output.exists())

    def test_real_formatter_uses_shared_default_without_rewriting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            project = root / "backend"
            project.mkdir()
            config = root / ".golangci.yaml"
            config.write_text('version: "2"\nformatters:\n  enable:\n    - gofmt\n')
            (project / "go.mod").write_text("module example.com/fixture\n\ngo 1.24\n")
            source = project / "main.go"
            source.write_text(
                "package main\n\nfunc main() {\n\tvar value interface{}\n\tprintln(value)\n}\n"
            )
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
            self.assertIn("any", result.stdout)
            self.assertEqual(source.read_bytes(), original)
            result = subprocess.run(
                ["bash", str(ACTION / "format.sh")],
                cwd=project,
                env=dict(os.environ, LINT_CONFIG="../.golangci.yaml"),
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(source.read_bytes(), original)
            self.assertEqual(
                config.read_text(), 'version: "2"\nformatters:\n  enable:\n    - gofmt\n'
            )
            subprocess.run(
                ["gofmt", "-w", "-r", "interface{} -> any", "main.go"], cwd=project, check=True
            )
            result = subprocess.run(
                ["bash", str(ACTION / "format.sh")],
                cwd=project,
                env=dict(os.environ, LINT_CONFIG=""),
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
