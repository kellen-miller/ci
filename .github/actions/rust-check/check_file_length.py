#!/usr/bin/env python3
"""Enforce the shared code-line limit for handwritten workspace Rust files."""

import json
import os
import subprocess
import sys
from pathlib import Path

MAX_LINES = 750


def rust_code_lines(source):
    """Count nonblank Rust lines outside comments, keeping multiline strings."""
    count = 0
    line_has_code = False
    index = 0
    block_depth = 0
    string_end = None
    raw_end = None
    escaped = False

    while index < len(source):
        character = source[index]
        following = source[index : index + 2]

        if character == "\n":
            count += line_has_code
            line_has_code = False
            if string_end is not None and escaped:
                escaped = False
            index += 1
            continue

        if string_end is not None or raw_end is not None:
            if raw_end is not None and source.startswith(raw_end, index):
                line_has_code = True
                index += len(raw_end)
                raw_end = None
                continue
            if string_end == '"' and escaped:
                escaped = False
            elif string_end == '"' and character == "\\":
                escaped = True
            elif string_end is not None and source.startswith(string_end, index):
                line_has_code = True
                index += len(string_end)
                string_end = None
                continue
            if not character.isspace():
                line_has_code = True
            index += 1
            continue

        if block_depth:
            if following == "/*":
                block_depth += 1
                index += 2
            elif following == "*/":
                block_depth -= 1
                index += 2
            else:
                index += 1
            continue

        if following == "//":
            newline = source.find("\n", index)
            index = len(source) if newline < 0 else newline
            continue
        if following == "/*":
            block_depth = 1
            index += 2
            continue

        raw_prefix = next(
            (prefix for prefix in ("br", "cr", "r") if source.startswith(prefix, index)),
            None,
        )
        if raw_prefix is not None:
            quote = index + len(raw_prefix)
            while quote < len(source) and source[quote] == "#":
                quote += 1
            if quote < len(source) and source[quote] == '"':
                line_has_code = True
                raw_end = '"' + "#" * (quote - index - len(raw_prefix))
                index = quote + 1
                continue

        if character == '"':
            line_has_code = True
            string_end = '"'
            index += 1
            continue

        if character == "'":
            line_has_code = True
            closing = index + 1
            if closing < len(source) and source[closing] == "\\":
                closing += 1
                if closing < len(source) and source[closing] == "u":
                    closing += 1
                    if closing < len(source) and source[closing] == "{":
                        brace = source.find("}", closing + 1)
                        closing = len(source) if brace < 0 else brace + 1
                elif closing < len(source) and source[closing] == "x":
                    closing += 3
                else:
                    closing += 1
            else:
                closing += 1
            if closing < len(source) and source[closing] == "'":
                index = closing + 1
            else:
                index += 1
            continue

        if not character.isspace():
            line_has_code = True
        index += 1

    return count + line_has_code


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
        source = path.read_bytes().decode("utf-8-sig").replace("\r\n", "\n")
    except (OSError, RuntimeError, UnicodeError) as error:
        raise SystemExit(
            f"Cannot read Rust source for file-length check: {path}: {error}"
        ) from None

    lines = rust_code_lines(source)
    if lines > MAX_LINES:
        violations.append((path, lines))

if violations:
    for path, lines in violations:
        print(f"Rust file exceeds {MAX_LINES} code lines: {path} ({lines} lines)", file=sys.stderr)
    raise SystemExit(1)
