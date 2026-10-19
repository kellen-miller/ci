#!/usr/bin/env bash
set -euo pipefail

WORKING_DIRECTORY="${WORKING_DIRECTORY-.}"
CHECKS="${CHECKS-fmt clippy test build}"
ALL_FEATURES="${ALL_FEATURES-true}"
BUILD_PROFILE="${BUILD_PROFILE-dev}"

for value in "$WORKING_DIRECTORY" "$CHECKS" "$ALL_FEATURES" "$BUILD_PROFILE"; do
  if [[ "$value" == *$'\n'* || "$value" == *$'\r'* ]]; then
    echo 'Rust check inputs must be single-line values' >&2
    exit 1
  fi
done

if [[ -z "$WORKING_DIRECTORY" ]]; then
  echo 'working-directory must not be empty' >&2
  exit 1
fi

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
if [[ "$ALL_FEATURES" == true ]]; then
  workspace_args+=(--all-features)
fi

cd -- "$WORKING_DIRECTORY"
for check in "${checks[@]}"; do
  case "$check" in
    fmt)
      cargo fmt --all -- --check
      ;;
    clippy)
      cargo clippy "${workspace_args[@]}" --all-targets -- -D warnings
      ;;
    test)
      cargo test "${workspace_args[@]}" --all-targets
      cargo test "${workspace_args[@]}" --doc
      ;;
    build)
      cargo build "${workspace_args[@]}" --profile "$BUILD_PROFILE"
      ;;
  esac
done
