#!/usr/bin/env bash
# Bootstrap only: verify a fixed release before handing all operation logic to Rust.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
export CI_ROOT="$root"
if [[ -n "${CI_BINARY:-}" ]]; then
  exec "$CI_BINARY" "$@"
fi
export CI_VERIFY_SOURCE=true

version="$(tr -d '\r\n' < "$root/runtime-version")"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid runtime version' >&2; exit 1; }
case "${RUNNER_OS:-$(uname -s)}/${RUNNER_ARCH:-$(uname -m)}" in
  Linux/X64|Linux/x86_64) target=x86_64-unknown-linux-musl ;;
  Linux/ARM64|Linux/aarch64) target=aarch64-unknown-linux-musl ;;
  macOS/X64|Darwin/x86_64) target=x86_64-apple-darwin ;;
  macOS/ARM64|Darwin/arm64) target=aarch64-apple-darwin ;;
  Windows/X64|MINGW*/x86_64) target=x86_64-pc-windows-msvc ;;
  *) echo 'Unsupported CI runtime platform' >&2; exit 1 ;;
esac
directory="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/personal-ci/$version/$target"
mkdir -p "$directory"
archive="ci-$target.tar.gz"
base="https://github.com/kellen-miller/ci/releases/download/ci-v$version"
if [[ ! -f "$directory/$archive" || ! -f "$directory/$archive.sha256" ]]; then
  curl --fail --silent --show-error --location --retry 3 "$base/$archive" -o "$directory/$archive"
  curl --fail --silent --show-error --location --retry 3 "$base/$archive.sha256" -o "$directory/$archive.sha256"
fi
expected="$(awk -v name="$archive" '$2 == name {print $1}' "$directory/$archive.sha256")"
[[ "$expected" =~ ^[0-9a-f]{64}$ ]] || { echo 'Invalid CI runtime checksum record' >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$directory/$archive" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "$directory/$archive" | awk '{print $1}')"
fi
[[ "$actual" == "$expected" ]] || { rm -f "$directory/$archive" "$directory/$archive.sha256"; echo 'CI runtime checksum mismatch' >&2; exit 1; }
executable=ci
[[ "$target" != *windows* ]] || executable=ci.exe
tar -xzf "$directory/$archive" -C "$directory" "$executable"
chmod +x "$directory/$executable"
exec "$directory/$executable" "$@"
