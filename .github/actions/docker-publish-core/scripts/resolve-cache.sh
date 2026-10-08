#!/usr/bin/env bash
set -euo pipefail

sanitize() {
  printf '%s' "$1" | tr -c '[:alnum:]_.-' '-'
}

ref_name="$(sanitize "${GITHUB_REF_NAME:-unknown}")"
case "${GITHUB_EVENT_NAME:-unknown}" in
  pull_request|pull_request_target)
    trust_scope="pr/$(sanitize "${GITHUB_HEAD_REF:-$ref_name}")"
    ;;
  release)
    trust_scope="release/$ref_name"
    ;;
  *)
    if [ "${GITHUB_REF_TYPE:-}" = "tag" ]; then
      trust_scope="release/$ref_name"
    else
      trust_scope="branch/$ref_name"
    fi
    ;;
esac

scope="$(sanitize "${REPO}/${trust_scope}")"
delimiter="cache_${RANDOM}_$$"
{
  echo "from<<${delimiter}"
  echo "${CACHE_FROM:-type=gha,scope=${scope}}"
  echo "$delimiter"
  echo "to<<${delimiter}"
  echo "${CACHE_TO:-type=gha,scope=${scope},mode=max}"
  echo "$delimiter"
} >> "$GITHUB_OUTPUT"
