#!/usr/bin/env bash
set -euo pipefail

if [[ ! "$REPO" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "repo must be owner/name" >&2
  exit 1
fi

if [ "${VERSION}" = "latest" ]; then
  tag=$(curl --fail --silent --show-error --retry 3 \
    "https://api.github.com/repos/${REPO}/releases/latest" | jq -er '.tag_name | select(type == "string" and length > 0)')
else
  tag="${VERSION}"
fi

if [[ -z "$tag" || "$tag" == *$'\n'* || "$tag" == *$'\r'* ]]; then
  echo "Release tag must be a nonempty single line" >&2
  exit 1
fi

echo "tag=${tag}" >> "$GITHUB_OUTPUT"
