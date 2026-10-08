#!/usr/bin/env python3
"""Resolve one package manager, its lockfile and the requested Node runtime."""

import json
import os
from pathlib import Path

package = json.loads(Path("package.json").read_text())
lockfiles = {
    "pnpm": ["pnpm-lock.yaml"],
    "npm": ["package-lock.json"],
    "yarn": ["yarn.lock"],
    "bun": ["bun.lock", "bun.lockb"],
}
manager = os.environ.get("INPUT_PM", "")
present = {name: [f for f in files if Path(f).is_file()] for name, files in lockfiles.items()}
if not manager:
    detected = [name for name, files in present.items() if files]
    if len(detected) != 1:
        raise SystemExit("Expected one package manager lockfile; set package-manager explicitly")

    manager = detected[0]

if manager not in present or not present[manager]:
    raise SystemExit(f"Unsupported package manager or missing lockfile: {manager}")

if len(present[manager]) != 1:
    raise SystemExit(f"Multiple {manager} lockfiles found; keep one")

version = os.environ.get("INPUT_NODE", "")
if not version:
    for name in (".node-version", ".nvmrc"):
        if Path(name).is_file():
            version = Path(name).read_text().strip()
            break

    version = version or package.get("engines", {}).get("node", "24")

lockfile = Path(os.environ["INPUT_WD"]) / present[manager][0]
values = {"package-manager": manager, "lockfile": lockfile.as_posix(), "node-version": version}
with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
    for key, value in values.items():
        if "\n" in value or "\r" in value:
            raise SystemExit(f"{key} must be a single-line value")

        output.write(f"{key}={value}\n")
