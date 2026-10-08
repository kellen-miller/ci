#!/usr/bin/env python3
"""Run the same validation locally and in CI."""

import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
commands = [
    ["ruff", "check", "."],
    ["ruff", "format", "--check", "."],
    ["yamllint", "."],
    ["actionlint"],
    ["shellcheck", *[str(path) for path in sorted((root / ".github/actions").rglob("*.sh"))]],
    [sys.executable, ".github/actions/validate-workflow-security/validate.py"],
    [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-v"],
]
for command in commands:
    print(f"Running {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=root, check=True)
