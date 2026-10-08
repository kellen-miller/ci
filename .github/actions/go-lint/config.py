#!/usr/bin/env python3
"""Encode a validated config path for golangci-lint-action's argument parser."""

import os
import shlex
from pathlib import Path

config = os.environ.get("LINT_CONFIG", "")
if "\n" in config or "\r" in config:
    raise SystemExit("config must be a single-line path")

if config and not Path(config).is_file():
    raise SystemExit(f"config file does not exist: {config}")

args = shlex.join(["--config", config]) if config else ""
with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
    output.write(f"args={args}\n")
