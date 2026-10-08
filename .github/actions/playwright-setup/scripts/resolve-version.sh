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
#   version, cache-key, cache-path

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

case "$RUNNER_OS" in
  macOS) cache_path="$HOME/Library/Caches/ms-playwright" ;;
  Windows) cache_path="${LOCALAPPDATA:?}/ms-playwright" ;;
  Linux) cache_path="${XDG_CACHE_HOME:-$HOME/.cache}/ms-playwright" ;;
  *)
    echo "::error::Unsupported Playwright runner OS: $RUNNER_OS"
    exit 1
    ;;
esac

{
  echo "version=$version"
  echo "cache-key=$cache_key"
  echo "cache-path=$cache_path"
} >> "$GITHUB_OUTPUT"
echo "::notice::Playwright $version (cache key: $cache_key)"
