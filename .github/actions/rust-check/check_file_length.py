#!/usr/bin/env python3
"""Enforce the shared physical-line limit for handwritten workspace Rust files."""

import json
import os
import subprocess
import sys
from pathlib import Path

MAX_LINES = 750

try:
    metadata = json.load(sys.stdin)
    workspace_members = set(metadata["workspace_members"])
    all_packages = metadata["packages"]
    target_directory = Path(metadata["target_directory"]).resolve()
except (KeyError, TypeError, json.JSONDecodeError) as error:
    raise SystemExit(
        f"Invalid Cargo workspace metadata for Rust file-length check: {error}"
    ) from None

member_roots = {}
dependency_roots = set()
for package in all_packages:
    try:
        manifest = Path(package["manifest_path"]).resolve(strict=package["id"] in workspace_members)
        if package["id"] not in workspace_members:
            dependency_roots.add(manifest.parent)
            continue

        if not manifest.is_file() or manifest.name != "Cargo.toml":
            raise SystemExit(f"Invalid Cargo manifest path in workspace metadata: {manifest}")

        package_metadata = package.get("metadata")
        ci_metadata = package_metadata.get("ci", {}) if isinstance(package_metadata, dict) else {}
        generated = isinstance(ci_metadata, dict) and ci_metadata.get("generated") is True
        member_roots[manifest.parent] = generated
    except (KeyError, OSError, TypeError, AttributeError) as error:
        raise SystemExit(
            f"Invalid Cargo package metadata for Rust file-length check: {error}"
        ) from None


def is_dependency_path(path, package_root):
    return any(
        root != package_root and root.is_relative_to(package_root) and path.is_relative_to(root)
        for root in dependency_roots
    )


def contains_rust_source(directory):
    for _, _, files in os.walk(
        directory,
        onerror=lambda error: sys.exit(
            f"Cannot inspect Rust source symlink target: {error.filename}: {error}"
        ),
    ):
        if any(name.endswith(".rs") for name in files):
            return True

    return False


for root, generated in member_roots.items():
    if generated:
        continue
    if not root.is_dir():
        raise SystemExit(f"Workspace package source directory is missing: {root}")

git_root = subprocess.run(
    ["git", "rev-parse", "--show-toplevel"],
    cwd=Path.cwd(),
    capture_output=True,
    check=False,
)
if git_root.returncode == 0:
    checkout = Path(os.fsdecode(git_root.stdout.rstrip(b"\n"))).resolve()
    outside_checkout = [
        root
        for root, generated in member_roots.items()
        if not generated and not root.is_relative_to(checkout)
    ]
    if outside_checkout:
        raise SystemExit(
            f"Workspace package source directory is outside the Git checkout: {outside_checkout[0]}"
        )

    listed_files = subprocess.run(
        [
            "git",
            "-C",
            str(checkout),
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        capture_output=True,
        check=False,
    )
    if listed_files.returncode != 0:
        raise SystemExit(
            f"Cannot list Rust source candidates from Git: {os.fsdecode(listed_files.stderr)}"
        )

    candidates = {checkout / os.fsdecode(name) for name in listed_files.stdout.split(b"\0") if name}
elif git_root.returncode == 128 and b"not a git repository" in git_root.stderr.lower():
    candidates = set()
    for root, generated in member_roots.items():
        if generated:
            continue

        for current, directories, files in os.walk(
            root,
            onerror=lambda error: sys.exit(
                f"Cannot scan Rust source directory: {error.filename}: {error}"
            ),
        ):
            current_path = Path(current)
            candidates.update(
                current_path / name for name in directories if (current_path / name).is_symlink()
            )
            directories[:] = sorted(
                directory
                for directory in directories
                if not (current_path / directory).resolve().is_relative_to(target_directory)
                and (current_path / directory).resolve() not in dependency_roots
                and not member_roots.get((current_path / directory).resolve(), False)
            )
            candidates.update(current_path / name for name in files if name.endswith(".rs"))
else:
    raise SystemExit(
        f"Cannot locate Git checkout for Rust source discovery: {os.fsdecode(git_root.stderr)}"
    )

violations = []
for path in sorted(candidates):
    package_roots = [root for root in member_roots if path.is_relative_to(root)]
    if not package_roots:
        continue

    package_root = max(package_roots, key=lambda root: len(root.parts))
    if member_roots[package_root] or path.is_relative_to(target_directory):
        continue
    if is_dependency_path(path, package_root):
        continue
    if not path.name.endswith(".rs") and not path.is_dir():
        continue

    if path.is_symlink():
        try:
            resolved_path = path.resolve(strict=True)
        except (OSError, RuntimeError) as error:
            raise SystemExit(f"Cannot resolve Rust source symlink: {path}: {error}") from None

        if resolved_path.is_relative_to(target_directory):
            continue
        if is_dependency_path(resolved_path, package_root):
            raise SystemExit(
                f"Rust source symlink points into a dependency package: {path} -> {resolved_path}"
            )

        resolved_owners = [root for root in member_roots if resolved_path.is_relative_to(root)]
        resolved_owner = (
            max(resolved_owners, key=lambda root: len(root.parts)) if resolved_owners else None
        )
        if resolved_owner is not None and member_roots[resolved_owner]:
            raise SystemExit(
                "Rust source symlink points into a generated workspace package: "
                f"{path} -> {resolved_path}"
            )
        if path.is_dir():
            if resolved_path.is_relative_to(package_root) or resolved_owner is not None:
                continue
            if contains_rust_source(resolved_path):
                raise SystemExit(
                    "Rust source directory symlink targets Rust files outside package ownership: "
                    f"{path} -> {resolved_path}"
                )
            continue

        if not resolved_path.is_relative_to(package_root) and resolved_owner is None:
            raise SystemExit(
                "Rust source symlink escapes workspace package ownership: "
                f"{path} -> {resolved_path}"
            )

    if not path.name.endswith(".rs"):
        continue
    if not path.is_file() and not path.is_symlink():
        continue

    try:
        source = path.read_text(encoding="utf-8")
    except (OSError, RuntimeError, UnicodeError) as error:
        raise SystemExit(
            f"Cannot read Rust source for file-length check: {path}: {error}"
        ) from None

    lines = source.count("\n") + int(bool(source) and not source.endswith("\n"))
    if lines > MAX_LINES:
        violations.append((path, lines))

if violations:
    for path, lines in violations:
        print(
            f"Rust file exceeds {MAX_LINES} physical lines: {path} ({lines} lines)", file=sys.stderr
        )
    raise SystemExit(1)
