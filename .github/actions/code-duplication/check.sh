#!/usr/bin/env bash
set -euo pipefail

config="${CPD_CONFIG:-.jscpd.json}"
if [[ ! -f "$config" ]]; then
  echo "Config file does not exist: $config" >&2
  exit 1
fi

args=(--config "$config" --no-tips --no-colors)
if [[ -n "${CPD_THRESHOLD:-}" ]]; then
  args+=(--threshold "$CPD_THRESHOLD")
fi

if [[ -n "${CPD_REPORTERS:-}" ]]; then
  exec cpd "${args[@]}" --reporters "$CPD_REPORTERS"
fi

status=0
cpd "${args[@]}" || status=$?
if [[ "$status" -ne 0 ]]; then
  cpd --config "$config" --no-tips --no-colors --reporters ai --threshold 100 || true
fi

exit "$status"
