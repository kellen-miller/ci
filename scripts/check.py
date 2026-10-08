#!/usr/bin/env python3
"""Run the same validation locally and in CI."""

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
    ["ruff", "check", "."],
    ["ruff", "format", "--check", "."],
    ["yamllint", "--strict", "."],
    ["actionlint"],
    ["golangci-lint", "config", "verify", "--config", "configs/golangci.yaml"],
    ["shellcheck", *sorted(set(shell_scripts))],
    [sys.executable, ".github/actions/validate-workflow-security/validate.py"],
    [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-v"],
    [
        "npm",
        "ci",
        "--prefix",
        ".github/actions/release/semantic-release",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
    ],
    ["npm", "test", "--prefix", ".github/actions/release/semantic-release"],
]
for command in commands:
    print(f"Running {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=root, check=True)
