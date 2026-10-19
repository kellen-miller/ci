import re
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
    def test_caller_inputs_and_shared_pinned_executor(self):
        executors = []
        for name, workflow in WORKFLOWS.items():
            with self.subTest(workflow=name):
                inputs = workflow["on"]["workflow_call"]["inputs"]
                job = workflow["jobs"][name]
                self.assertEqual(inputs["working-directory"]["default"], ".")
                self.assertEqual(inputs["runner"]["default"], "ubuntu-latest")
                self.assertEqual(inputs["toolchain"]["default"], "")
                self.assertEqual(job["runs-on"], "${{ inputs.runner }}")
                self.assertEqual(workflow["permissions"], {"contents": "read"})
                checkout, executor = job["steps"]
                self.assertEqual(checkout["with"]["persist-credentials"], "false")
                self.assertEqual(
                    executor["with"]["working-directory"], "${{ inputs.working-directory }}"
                )
                self.assertEqual(executor["with"]["toolchain"], "${{ inputs.toolchain }}")
                self.assertRegex(
                    executor["uses"],
                    re.compile(r"^kellen-miller/ci/\.github/actions/rust-check@[a-f0-9]{40}$"),
                )
                executors.append(executor["uses"])
        self.assertEqual(executors[0], executors[1])

    def test_lint_selection_and_optional_build_profiles(self):
        lint = WORKFLOWS["lint"]["jobs"]["lint"]["steps"][-1]
        self.assertEqual(lint["with"]["checks"], "fmt clippy")
        test = WORKFLOWS["test"]
        inputs = test["on"]["workflow_call"]["inputs"]
        for name in ("build", "build-release"):
            self.assertEqual(inputs[name]["type"], "boolean")
            self.assertEqual(inputs[name]["default"], "false")
        executor = test["jobs"]["test"]["steps"][-1]
        self.assertEqual(
            executor["with"]["checks"], "${{ inputs.build && 'test build' || 'test' }}"
        )
        self.assertEqual(
            executor["with"]["build-profile"],
            "${{ inputs.build-release && 'release' || 'dev' }}",
        )
