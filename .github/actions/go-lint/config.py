#!/usr/bin/env python3
"""Resolve the shared Go policy or an explicit project override."""

import argparse
import os
import shlex
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--github-output", action="store_true")
options = parser.parse_args()

config = os.environ.get("LINT_CONFIG", "")
if "\n" in config or "\r" in config:
    raise SystemExit("config must be a single-line path")

config_path = (
    Path(config) if config else Path(__file__).resolve().parents[3] / "config/golangci.yaml"
).resolve()
if "\n" in str(config_path) or "\r" in str(config_path):
    raise SystemExit("config must be a single-line path")

if not config_path.is_file():
    raise SystemExit(f"config file does not exist: {config_path}")

if options.github_output:
    args = shlex.join(["--config", str(config_path)])
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"args={args}\n")

print(config_path)
