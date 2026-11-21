#!/usr/bin/env bash
# Download a GNU release archive and verify its pinned SHA-256.
#
# Usage: fetch-gnu-release.sh <package>/<file> <sha256> <output>
#
# Tries ftp.gnu.org first, then the ftpmirror.gnu.org redirector. A candidate
# replaces <output> only after it matches the pinned hash, so a mirror can make
# the download available but can never change what is built.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: fetch-gnu-release.sh <package>/<file> <sha256> <output>" >&2
  exit 2
fi
release_path="$1"
sha256="$2"
output="$3"
if [[ ! "$sha256" =~ ^[0-9a-f]{64}$ ]]; then
  echo "fetch-gnu-release: invalid pinned SHA-256 for $release_path: $sha256" >&2
  exit 2
fi

mkdir -p "$(dirname "$output")"
temporary=$(mktemp "$output.download.XXXXXX")
trap 'rm -f "$temporary"' EXIT

for url in "https://ftp.gnu.org/gnu/$release_path" "https://ftpmirror.gnu.org/$release_path"; do
  # A short connect timeout moves to the mirror quickly when a host is down,
  # instead of waiting for the OS connect timeout on every retry.
  if ! curl --connect-timeout 20 --retry 3 --retry-all-errors -fSL "$url" -o "$temporary"; then
    echo "fetch-gnu-release: download failed from $url" >&2
    continue
  fi
  if printf '%s  %s\n' "$sha256" "$temporary" | sha256sum -c - >/dev/null 2>&1; then
    mv "$temporary" "$output"
    trap - EXIT
    exit 0
  fi
  echo "fetch-gnu-release: SHA-256 mismatch for $url (expected $sha256)" >&2
done

echo "fetch-gnu-release: no GNU source served $release_path with SHA-256 $sha256" >&2
exit 1
