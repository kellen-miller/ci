#!/usr/bin/env bash
set -euo pipefail

args=(fmt --diff)
if [[ -n "${LINT_CONFIG:-}" ]]; then
  args+=(--config "$LINT_CONFIG")
fi

diff_file="$(mktemp)"
trap 'rm -f "$diff_file"' EXIT
status=0
golangci-lint "${args[@]}" > "$diff_file" || status=$?
cat "$diff_file"
if [[ "$status" -ne 0 ]] || [[ -s "$diff_file" ]]; then
  exit 1
fi
