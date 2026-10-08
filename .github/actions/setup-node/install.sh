#!/usr/bin/env bash
set -euo pipefail

case "$PM" in
  pnpm)
    args=(pnpm install --frozen-lockfile)
    if [[ -n "${PNPM_MINIMUM_RELEASE_AGE:-}" ]]; then
      args+=("--config.minimum-release-age=$PNPM_MINIMUM_RELEASE_AGE")
    fi

    if [[ -n "${PNPM_STRICT_DEP_BUILDS:-}" ]]; then
      args+=("--config.strict-dep-builds=$PNPM_STRICT_DEP_BUILDS")
    fi

    ;;
  npm) args=(npm ci) ;;
  yarn)
    version="$(yarn --version)"
    if [[ "${version%%.*}" -ge 2 ]]; then
      args=(yarn install --immutable)
    else
      args=(yarn install --frozen-lockfile)
    fi
    ;;
  bun) args=(bun install --frozen-lockfile) ;;
  *) echo "Unsupported package manager: $PM" >&2; exit 1 ;;
esac

if [[ -n "${EXTRA_ARGS:-}" ]]; then
  read -r -a extra <<< "$EXTRA_ARGS"
  if [[ ${#extra[@]} -gt 0 ]]; then
    args+=("${extra[@]}")
  fi
fi

"${args[@]}"
