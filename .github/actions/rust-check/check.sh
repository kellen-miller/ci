#!/usr/bin/env bash
set -euo pipefail

SETUP_TOOLCHAIN="${SETUP_TOOLCHAIN-true}"
WORKING_DIRECTORY="${WORKING_DIRECTORY-.}"
CHECKS="${CHECKS-fmt clippy test build}"
ALL_FEATURES="${ALL_FEATURES-true}"
BUILD_PROFILE="${BUILD_PROFILE-dev}"
POLICY_DIRECTORY="${POLICY_DIRECTORY-}"

for value in "$SETUP_TOOLCHAIN" "$WORKING_DIRECTORY" "$CHECKS" "$ALL_FEATURES" "$BUILD_PROFILE" "$POLICY_DIRECTORY"; do
  if [[ "$value" == *$'\n'* || "$value" == *$'\r'* ]]; then
    echo 'Rust check inputs must be single-line values' >&2
    exit 1
  fi
done

if [[ -z "$WORKING_DIRECTORY" ]]; then
  echo 'working-directory must not be empty' >&2
  exit 1
fi

case "$SETUP_TOOLCHAIN" in
  true|false) ;;
  *) echo 'setup-toolchain must be true or false' >&2; exit 1 ;;
esac

case "$ALL_FEATURES" in
  true|false) ;;
  *) echo 'all-features must be true or false' >&2; exit 1 ;;
esac

case "$BUILD_PROFILE" in
  dev|release) ;;
  *) echo 'build-profile must be dev or release' >&2; exit 1 ;;
esac

IFS=' ' read -r -a checks <<< "$CHECKS"
if [[ "${#checks[@]}" -eq 0 ]]; then
  echo 'checks must contain at least one check' >&2
  exit 1
fi

selected_checks=' '
for check in "${checks[@]}"; do
  case "$check" in
    fmt|clippy|test|build) ;;
    *) echo 'checks must contain only fmt, clippy, test or build' >&2; exit 1 ;;
  esac

  if [[ "$selected_checks" == *" $check "* ]]; then
    echo 'checks must not contain duplicates' >&2
    exit 1
  fi

  selected_checks+="$check "
done

workspace_args=(--locked --workspace)
metadata_args=(--locked --no-deps --format-version 1)
if [[ "$ALL_FEATURES" == true ]]; then
  workspace_args+=(--all-features)
  metadata_args+=(--all-features)
fi

script_directory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$WORKING_DIRECTORY"

if [[ "$selected_checks" == *' fmt '* || "$selected_checks" == *' clippy '* ]]; then
  policy_directory="${POLICY_DIRECTORY:-$script_directory/../../../config/rust}"
  policy_directory="$(cd -- "$policy_directory" && pwd)"
fi

if [[ "$selected_checks" == *' fmt '* && ! -f "$policy_directory/rustfmt.toml" ]]; then
  echo "Missing Rust formatting policy: $policy_directory/rustfmt.toml" >&2
  exit 1
fi

lint_args=()
if [[ "$selected_checks" == *' clippy '* ]]; then
  if [[ ! -f "$policy_directory/clippy.toml" ]]; then
    echo "Missing Clippy configuration: $policy_directory/clippy.toml" >&2
    exit 1
  fi

  lint_flags="$(python3 - "$policy_directory/lints.toml" <<'PY'
import sys
import tomllib
from pathlib import Path

policy = tomllib.loads(Path(sys.argv[1]).read_text())
levels = {"allow": "-A", "warn": "-W", "deny": "-D", "forbid": "-F"}
if set(policy) != {"rust", "clippy"} or not all(policy.values()):
    raise SystemExit("Rust policy must contain nonempty rust and clippy lint tables")

for namespace, lints in policy.items():
    for name, level in lints.items():
        if level not in levels or not name.isidentifier():
            raise SystemExit(f"Invalid Rust lint policy entry: {namespace}.{name}")

        print(levels[level])
        print(f"clippy::{name}" if namespace == "clippy" else name)
PY
)"
  while IFS= read -r argument; do
    lint_args+=("$argument")
  done <<< "$lint_flags"
fi

for check in "${checks[@]}"; do
  case "$check" in
    fmt)
      cargo fmt --all -- --check --config-path "$policy_directory/rustfmt.toml"
      ;;
    clippy)
      clippy_selection=$(cargo metadata "${metadata_args[@]}" | python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
members = set(metadata["workspace_members"])
packages = [package for package in metadata["packages"] if package["id"] in members]
generated = []
for package in packages:
    ci = (package.get("metadata") or {}).get("ci", {})
    if isinstance(ci, dict) and ci.get("generated") is True:
        generated.append(package["name"])

print(len(packages) - len(generated), *generated)
')
      read -r -a clippy_packages <<< "$clippy_selection"
      if [[ "${clippy_packages[0]}" == 0 ]]; then
        echo 'Skipping Clippy: all workspace packages declare package.metadata.ci.generated = true'
        continue
      fi

      clippy_args=("${workspace_args[@]}")
      if [[ "${#clippy_packages[@]}" -gt 1 ]]; then
        # Excluded path dependencies still compile, but must not receive Clippy lints.
        clippy_args+=(--no-deps)
      fi

      for package in "${clippy_packages[@]:1}"; do
        clippy_args+=(--exclude "$package")
      done

      CLIPPY_CONF_DIR="$policy_directory" cargo clippy "${clippy_args[@]}" --all-targets -- "${lint_args[@]}" -D warnings
      ;;
    test)
      cargo test "${workspace_args[@]}" --all-targets
      has_doctests=$(cargo metadata "${metadata_args[@]}" | python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
members = set(metadata["workspace_members"])
print(str(any(
    target["doctest"]
    for package in metadata["packages"] if package["id"] in members
    for target in package["targets"]
)).lower())
')
      if [[ "$has_doctests" == true ]]; then
        cargo test "${workspace_args[@]}" --doc
      fi
      ;;
    build)
      cargo build "${workspace_args[@]}" --all-targets --profile "$BUILD_PROFILE"
      ;;
  esac
done
