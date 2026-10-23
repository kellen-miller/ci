#!/usr/bin/env bash
# External shell entry point used to qualify literal arguments and exit status.
set -euo pipefail

if [[ $1 == --fail ]]; then
  exit 33
fi

printf '%s\n' "$@"
