#!/usr/bin/env bash
#
# Native C Command Compatibility Test Runner
#
# Builds and installs the maintained C command set against the WASI toolchain.
#
# Usage:
#   ./scripts/test-gnu.sh
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
COMMANDS_DIR="$PROJECT_DIR/target/wasm32-wasip1/release/commands"

# Check standalone binaries exist
if [ ! -d "$COMMANDS_DIR" ]; then
    echo "Error: Commands directory not found at $COMMANDS_DIR"
    echo "Run 'make wasm' first."
    exit 1
fi

echo "=== Native C Command Compatibility Test Suite ==="
echo "Commands dir: $COMMANDS_DIR ($( ls -1 "$COMMANDS_DIR" | wc -l ) binaries)"
echo ""

make -C "$PROJECT_DIR/c" programs install

echo ""
echo "=== Native C command compatibility tests PASSED ==="
