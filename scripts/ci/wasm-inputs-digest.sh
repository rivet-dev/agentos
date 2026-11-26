#!/usr/bin/env bash
# Print a digest of every tracked file that can change the WASM command build
# outputs: the owned toolchain, per-package native sources, and the scripts that
# stage packaging inputs from the build. Tests and docs are excluded so editing
# them never forces a rebuild. Uses git blob ids, so it reads no file contents.
set -euo pipefail
git ls-files -s -- \
  toolchain \
  'software/*/native' \
  software/default-software.json \
  software/git/scripts/stage-templates.mjs \
  software/vim/scripts/stage-runtime.mjs |
  grep -vE '\.test\.(ts|mjs)$|\.md$' |
  sha256sum | cut -c1-40
