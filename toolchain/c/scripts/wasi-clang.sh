#!/bin/bash
set -euo pipefail

# SDK25's --no-wasm-opt accidentally suppresses linking altogether. Limit
# executable discovery for this compiler process instead: Clang locates its
# own wasm-ld beside itself, while Binaryen on the host PATH stays invisible.
# Keep the build system's PATH unchanged for configure, make, and other tools.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SDK_BIN="$SCRIPT_DIR/../vendor/wasi-sdk/bin"
export PATH="$SDK_BIN"
exec "$SDK_BIN/clang" "$@"
