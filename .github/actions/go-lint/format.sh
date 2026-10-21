#!/usr/bin/env bash
set -euo pipefail

config="$(python3 "$(dirname -- "${BASH_SOURCE[0]}")/config.py")"

diff_file="$(mktemp)"
trap 'rm -f "$diff_file"' EXIT
status=0
golangci-lint fmt --diff --config "$config" > "$diff_file" || status=$?
cat "$diff_file"
if [[ "$status" -ne 0 ]] || [[ -s "$diff_file" ]]; then
  exit 1
fi
