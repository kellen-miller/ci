"""Select CI groups from the complete PR diff; other events select all groups."""

import fnmatch
import json
import os
import subprocess
from pathlib import Path


def select_groups(filters, paths):
    return {
        group: any(
            fnmatch.fnmatchcase(path, pattern)
            or (pattern.startswith("**/") and fnmatch.fnmatchcase(path, pattern[3:]))
            for path in paths
            for pattern in patterns
        )
        for group, patterns in filters.items()
    }


def main():
    filters = json.loads(os.environ["FILTERS"])
    if not isinstance(filters, dict) or not filters:
        raise ValueError("filters must be a nonempty JSON object")

    for group, patterns in filters.items():
        if not group or not isinstance(patterns, list) or not patterns:
            raise ValueError(f"{group}: provide a nonempty list of glob patterns")

        if any(
            not isinstance(pattern, str) or not pattern or pattern.startswith("!")
            for pattern in patterns
        ):
            raise ValueError(f"{group}: only positive, nonempty glob patterns are supported")

    if os.environ["EVENT_NAME"] == "pull_request":
        base = os.environ["BASE_SHA"]
        head = os.environ["HEAD_SHA"]
        if not base or not head:
            raise ValueError("PR base and head SHAs are required")

        diff = subprocess.check_output(
            ["git", "diff", "--name-only", "--no-renames", "-z", f"{base}...{head}", "--"]
        )
        paths = [os.fsdecode(path) for path in diff.split(b"\0") if path]
        selected = select_groups(filters, paths)
    else:
        selected = dict.fromkeys(filters, True)

    changes = json.dumps(selected, separators=(",", ":"))
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"changes={changes}\n")

    print(f"CI groups: {changes}")


if __name__ == "__main__":
    main()
