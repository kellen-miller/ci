#!/usr/bin/env bash
#
# Resolve the Playwright version and emit a cache key covering the
# runner OS, Playwright version, and browser set.
#
# Required env:
#   BROWSERS            — space-separated browser list, optional (empty = all)
#   RUNNER_OS           — GitHub runner OS (Linux, macOS, Windows)
#   PLAYWRIGHT_VERSION  — explicit version override, optional. When unset the
#                         script sniffs ./node_modules/.bin/playwright instead.
# Writes to $GITHUB_OUTPUT:
#   version, cache-key

set -euo pipefail

version="${PLAYWRIGHT_VERSION:-}"
if [ -z "$version" ]; then
  if [ ! -x ./node_modules/.bin/playwright ]; then
    echo "::error::Playwright binary not found at ./node_modules/.bin/playwright. Install @playwright/test or playwright as a dependency, or pass 'playwright-version' explicitly."
    exit 1
  fi
  version="$(./node_modules/.bin/playwright --version | awk '{print $2}')"
fi

browsers_key="$(printf '%s' "${BROWSERS:-}" | tr -s '[:space:]' '-' | sed 's/^-//;s/-$//')"
[ -z "$browsers_key" ] && browsers_key="all"

cache_key="playwright-${RUNNER_OS}-${RUNNER_ARCH:?}-${version}-${browsers_key}"

{
  echo "version=$version"
  echo "cache-key=$cache_key"
} >> "$GITHUB_OUTPUT"
echo "::notice::Playwright $version (cache key: $cache_key)"
