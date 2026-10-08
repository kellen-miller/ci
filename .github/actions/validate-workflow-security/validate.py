#!/usr/bin/env python3
"""Validate workflow references, credential persistence and explicit secret boundaries."""

import argparse
import os
import re
from pathlib import Path

import yaml


def validate_document(document, location, errors, *, allow_secret_inheritance):
    if isinstance(document, list):
        for index, value in enumerate(document):
            validate_document(
                value,
                f"{location}[{index}]",
                errors,
                allow_secret_inheritance=allow_secret_inheritance,
            )

        return

    if not isinstance(document, dict):
        return

    reference = document.get("uses")
    if isinstance(reference, str):
        if reference.startswith("docker://"):
            if not re.search(r"@sha256:[0-9a-f]{64}$", reference):
                errors.append(f"{location}: Docker action must use a SHA-256 digest")

        elif not reference.startswith("./") and not re.search(r"@[0-9a-f]{40}$", reference):
            errors.append(f"{location}: action/workflow must use a full commit SHA: {reference}")

        if reference.startswith("actions/checkout@"):
            settings = document.get("with", {})
            if not isinstance(settings, dict) or settings.get("persist-credentials") != "false":
                errors.append(f"{location}: checkout must set persist-credentials: false")

    if document.get("secrets") == "inherit" and not allow_secret_inheritance:
        errors.append(
            f"{location}: pass named secrets explicitly instead of inheriting all secrets"
        )

    for key, value in document.items():
        validate_document(
            value,
            f"{location}.{key}",
            errors,
            allow_secret_inheritance=allow_secret_inheritance,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--allow-pull-request-target", action="store_true")
    parser.add_argument("--allow-secret-inheritance", action="store_true")
    args = parser.parse_args()
    errors = []
    files = []
    for directory in (args.root / ".github/workflows", args.root / ".github/actions"):
        for current, children, names in os.walk(directory):
            children[:] = [
                name for name in children if name not in {"node_modules", ".venv", ".git"}
            ]
            files.extend(Path(current) / name for name in names if name.endswith((".yaml", ".yml")))

    if not files:
        parser.error("no workflow or action YAML files found")

    for path in sorted(files):
        location = str(path.relative_to(args.root))
        try:
            # BaseLoader preserves 'on' as a key and normalizes scalar values to strings.
            document = yaml.load(path.read_text(), Loader=yaml.BaseLoader)
        except yaml.YAMLError as error:
            errors.append(f"{location}: invalid YAML: {error}")
            continue

        if not isinstance(document, dict):
            errors.append(f"{location}: expected a YAML mapping")
            continue

        events = document.get("on", {})
        if "pull_request_target" in events and not args.allow_pull_request_target:
            errors.append(f"{location}: pull_request_target requires explicit policy opt-in")

        validate_document(
            document,
            location,
            errors,
            allow_secret_inheritance=args.allow_secret_inheritance,
        )

    for error in errors:
        print(error)

    if errors:
        return 1

    print(f"Workflow security checks passed ({len(files)} files).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
