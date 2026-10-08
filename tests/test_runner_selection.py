import importlib.util
import io
import json
import os
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / ".github/actions/select-runner/select_runner.py"
SPEC = importlib.util.spec_from_file_location("select_runner", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RunnerSelectionTests(unittest.TestCase):
    def test_online_idle_label_match_in_paginated_results(self):
        busy = {"status": "online", "busy": True, "labels": [{"name": "home"}]}
        idle = {"status": "online", "busy": False, "labels": [{"name": "home"}]}
        responses = [
            io.BytesIO(json.dumps({"runners": runners}).encode())
            for runners in [[busy] * 100, [idle]]
        ]
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            with (
                patch.dict(
                    os.environ,
                    {
                        "PREFERRED_RUNNER": "home",
                        "FALLBACK_RUNNER": "ubuntu-24.04",
                        "RUNNER_STATUS_TOKEN": "fixture",
                        "GITHUB_REPOSITORY": "example/project",
                        "GITHUB_OUTPUT": str(output),
                    },
                ),
                patch.object(MODULE.urllib.request, "urlopen", side_effect=responses) as request,
            ):
                MODULE.main()

            self.assertEqual(output.read_text(), "runner=home\nself-hosted=true\n")
            self.assertEqual(request.call_count, 2)

    def test_offline_busy_or_unmatched_runners_use_hosted_capacity(self):
        runners = [
            {"status": "offline", "busy": False, "labels": [{"name": "home"}]},
            {"status": "online", "busy": True, "labels": [{"name": "home"}]},
            {"status": "online", "busy": False, "labels": [{"name": "other"}]},
        ]
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            with (
                patch.dict(
                    os.environ,
                    {
                        "PREFERRED_RUNNER": "home",
                        "FALLBACK_RUNNER": "ubuntu-24.04",
                        "RUNNER_STATUS_TOKEN": "fixture",
                        "GITHUB_REPOSITORY": "example/project",
                        "GITHUB_OUTPUT": str(output),
                    },
                ),
                patch.object(
                    MODULE.urllib.request,
                    "urlopen",
                    return_value=io.BytesIO(json.dumps({"runners": runners}).encode()),
                ),
            ):
                MODULE.main()

            self.assertEqual(output.read_text(), "runner=ubuntu-24.04\nself-hosted=false\n")

    def test_status_permission_failure_falls_back_without_exposing_the_token(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            with (
                patch.dict(
                    os.environ,
                    {
                        "PREFERRED_RUNNER": "home",
                        "FALLBACK_RUNNER": "ubuntu-24.04",
                        "RUNNER_STATUS_TOKEN": "fixture-secret",
                        "GITHUB_REPOSITORY": "example/project",
                        "GITHUB_OUTPUT": str(output),
                    },
                ),
                patch.object(
                    MODULE.urllib.request,
                    "urlopen",
                    side_effect=urllib.error.HTTPError(
                        "https://api.github.com", 403, "Forbidden", {}, None
                    ),
                ),
                patch("sys.stdout", new_callable=io.StringIO) as messages,
            ):
                MODULE.main()

            self.assertEqual(output.read_text(), "runner=ubuntu-24.04\nself-hosted=false\n")
            self.assertIn("HTTP 403", messages.getvalue())
            self.assertNotIn("fixture-secret", messages.getvalue())
