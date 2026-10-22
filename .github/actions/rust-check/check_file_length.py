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
    packages = [package for package in all_packages if package["id"] in workspace_members]
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
    if any(path.is_relative_to(root) for root in dependency_roots):
        continue

    if path.is_symlink() and path.is_dir():
        raise SystemExit(f"Rust source directory symlink cannot be scanned safely: {path}")
    if not path.name.endswith(".rs"):
        continue
    if not path.is_file() and not path.is_symlink():
        continue

    try:
        resolved_path = path.resolve(strict=True)
        if not resolved_path.is_relative_to(package_root):
            raise SystemExit(
                f"Rust source symlink escapes its workspace package: {path} -> {resolved_path}"
            )
        if resolved_path.is_relative_to(target_directory):
            raise SystemExit(
                f"Rust source symlink points into Cargo target output: {path} -> {resolved_path}"
            )
        if any(resolved_path.is_relative_to(root) for root in dependency_roots):
            raise SystemExit(
                f"Rust source symlink crosses into a dependency package: {path} -> {resolved_path}"
            )
        if any(
            root != package_root and resolved_path.is_relative_to(root) for root in member_roots
        ):
            raise SystemExit(
                "Rust source symlink crosses workspace package ownership: "
                f"{path} -> {resolved_path}"
            )

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
