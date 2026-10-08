import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / ".github/actions/helm-lint-charts/lint.sh"


class HelmLintTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.charts = self.root / "kubernetes/charts"
        self.chart = self.charts / "example"
        (self.chart / "templates").mkdir(parents=True)
        (self.chart / "Chart.yaml").write_text("apiVersion: v2\nname: example\nversion: 0.1.0\n")
        (self.chart / "values.yaml").write_text("message: default\n")
        (self.chart / "values-dev.yaml").write_text("message: development\n")
        (self.chart / "templates/config.yaml").write_text(
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: fixture\ndata:\n"
            "  message: {{ .Values.message | quote }}\n"
        )
        self.rendered = self.root / "rendered"

    def run_lint(self):
        return subprocess.run(
            ["bash", str(SCRIPT)],
            env=dict(
                os.environ,
                CHARTS_ROOT=str(self.charts),
                VALUES_PATTERN="values-*.yaml",
                RENDERED_DIR=str(self.rendered),
                VALIDATE_SCHEMAS="false",
            ),
            capture_output=True,
            text=True,
            check=False,
        )

    def test_default_style_root_checks_default_and_environment_values(self):
        result = self.run_lint()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        manifests = list(self.rendered.glob("*.yaml"))
        self.assertEqual(len(manifests), 2)
        text = "\n".join(path.read_text() for path in manifests)
        self.assertIn('message: "default"', text)
        self.assertIn('message: "development"', text)

    def test_empty_root_fails(self):
        self.charts = self.root / "empty"
        self.charts.mkdir()
        result = self.run_lint()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No charts found", result.stderr)

    def test_broken_template_fails_before_scan(self):
        (self.chart / "templates/broken.yaml").write_text('{{ fail "broken fixture" }}')
        result = self.run_lint()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("broken fixture", result.stdout + result.stderr)

    def test_vendored_chart_is_not_linted_independently(self):
        dependency = self.chart / "charts/dependency"
        dependency.mkdir(parents=True)
        (dependency / "Chart.yaml").write_text("apiVersion: v2\nname: dependency\nversion: 0.1.0\n")
        with (self.chart / "Chart.yaml").open("a") as metadata:
            metadata.write(
                "dependencies:\n  - name: dependency\n    version: 0.1.0\n"
                "    repository: file://charts/dependency\n"
            )

        result = self.run_lint()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(len(list(self.rendered.glob("*.yaml"))), 2)
