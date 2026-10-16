#!/usr/bin/env python3
"""Run the same validation locally and in CI."""

import json
import os
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
shell_scripts = [
    path
    for path in subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", ".github/actions"],
        cwd=root,
        text=True,
    ).splitlines()
    if path.endswith(".sh")
]
commands = [
    ("python", ["ruff", "check", "."]),
    ("python", ["ruff", "format", "--check", "."]),
    ("yaml", ["yamllint", "--strict", "."]),
    ("workflows", ["actionlint"]),
    ("go_config", ["golangci-lint", "config", "verify", "--config", "config/golangci.yaml"]),
    (
        "renovate",
        [
            "npx",
            "--yes",
            "--package",
            "renovate@44.145.3",
            "--",
            "renovate-config-validator",
            "--strict",
            "--no-global",
            "default.json",
            "renovate.json",
        ],
    ),
    ("shell", ["shellcheck", *sorted(set(shell_scripts))]),
    ("workflows", [sys.executable, ".github/actions/validate-workflow-security/validate.py"]),
    ("python", [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-v"]),
    (
        "release",
        [
            "npm",
            "ci",
            "--prefix",
            ".github/actions/release/semantic-release",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
        ],
    ),
    ("release", ["npm", "test", "--prefix", ".github/actions/release/semantic-release"]),
]
selected = json.loads(os.environ.get("CI_CHANGED_GROUPS") or "null")
for group, command in commands:
    if selected is not None and not selected[group]:
        print(f"Skipping {group}: inputs unchanged", flush=True)
        continue

    print(f"Running {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=root, check=True)
