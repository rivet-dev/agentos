#!/bin/bash
set -euo pipefail

# CMake passes the compiler as the first launcher argument. Only that SDK's
# own tools should be discoverable while Clang builds and links WASM.
compiler="$1"
shift
export PATH="${compiler%/*}"
exec "$compiler" "$@"
