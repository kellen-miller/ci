#!/usr/bin/env bash
set -euo pipefail

version="$KUBECONFORM_VERSION"
if [[ ! "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Expected an explicit kubeconform release such as v0.8.0" >&2
  exit 1
fi

case "$(uname -s)" in
  Linux) platform=linux ;;
  Darwin) platform=darwin ;;
  *) echo "Unsupported operating system" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) architecture=amd64 ;;
  arm64|aarch64) architecture=arm64 ;;
  *) echo "Unsupported architecture" >&2; exit 1 ;;
esac

archive="kubeconform-$platform-$architecture.tar.gz"
base="https://github.com/yannh/kubeconform/releases/download/$version"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT
curl --fail --silent --show-error --location "$base/$archive" -o "$temp_dir/$archive"
curl --fail --silent --show-error --location "$base/CHECKSUMS" -o "$temp_dir/CHECKSUMS"

python3 - "$temp_dir" "$archive" <<'PY'
import hashlib
import sys
from pathlib import Path

directory, archive = sys.argv[1:]
directory = Path(directory)
expected = None
for line in (directory / "CHECKSUMS").read_text().splitlines():
    fields = line.split()
    if len(fields) == 2 and fields[1].lstrip("*") == archive:
        expected = fields[0]
        break

actual = hashlib.sha256((directory / archive).read_bytes()).hexdigest()
if expected is None or actual != expected:
    raise SystemExit(f"Checksum verification failed for {archive}")
PY

tar -xzf "$temp_dir/$archive" -C "$temp_dir" kubeconform
install_dir="${KUBECONFORM_INSTALL_DIR:-$RUNNER_TEMP/kubeconform-bin}"
mkdir -p "$install_dir"
install -m 755 "$temp_dir/kubeconform" "$install_dir/kubeconform"
if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "$install_dir" >> "$GITHUB_PATH"
fi

"$install_dir/kubeconform" -v
