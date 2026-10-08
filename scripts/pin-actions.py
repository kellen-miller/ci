#!/usr/bin/env python3
"""Pin this repository's reusable workflows to a committed action snapshot."""

import argparse
import re
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("commit", help="Committed revision containing the action implementations")
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
commit = subprocess.check_output(
    ["git", "rev-parse", "--verify", f"{args.commit}^{{commit}}"], cwd=root, text=True
).strip()
if subprocess.check_output(["git", "diff", commit, "--", ".github/actions"], cwd=root):
    raise SystemExit("Commit action changes before pinning workflows")

reference = re.compile(r"kellen-miller/ci/(\.github/actions/[^@\s]+)@[0-9a-f]{40}")
for workflow in (root / ".github/workflows").glob("*.yaml"):
    text = workflow.read_text()
    for match in reference.finditer(text):
        subprocess.run(
            ["git", "cat-file", "-e", f"{commit}:{match.group(1)}/action.yaml"],
            cwd=root,
            check=True,
        )

    workflow.write_text(reference.sub(rf"kellen-miller/ci/\g<1>@{commit}", text))

print(f"Pinned shared actions to {commit}")
