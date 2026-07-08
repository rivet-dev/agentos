#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
Usage:
  CODEX_REPO=/path/to/codex-rs/codex-rs toolchain/scripts/build-codex-wasi.sh

Builds the external Codex WASI fork and installs the real codex-exec artifact
into software/codex/wasm/{codex-exec,codex}, where @agentos-software/codex-cli
stages commands from.

Env:
  CODEX_REPO   Required. Checkout of the Codex fork with scripts/build-wasi-codex-exec.sh.
  WASI_SDK_DIR Optional. Defaults to toolchain/c/vendor/wasi-sdk.
  DEST_DIR     Optional. Defaults to software/codex/wasm.
USAGE
}

if [ "${1:-}" = "--help" ]; then
	usage
	exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLCHAIN_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
AGENTOS_ROOT="$(cd "$TOOLCHAIN_DIR/.." && pwd)"

CODEX_REPO="${CODEX_REPO:-}"
WASI_SDK_DIR="${WASI_SDK_DIR:-$TOOLCHAIN_DIR/c/vendor/wasi-sdk}"
DEST_DIR="${DEST_DIR:-$AGENTOS_ROOT/software/codex/wasm}"

if [ -z "$CODEX_REPO" ]; then
	echo "ERROR: CODEX_REPO is required." >&2
	echo "       Set it to the wasi-port-codex-core checkout, for example:" >&2
	echo "       CODEX_REPO=/path/to/codex-rs/codex-rs make -C toolchain codex-required" >&2
	exit 1
fi

BUILD_SCRIPT="$CODEX_REPO/scripts/build-wasi-codex-exec.sh"
if [ ! -x "$BUILD_SCRIPT" ]; then
	echo "ERROR: codex fork build script not found or not executable: $BUILD_SCRIPT" >&2
	exit 1
fi

if [ ! -x "$WASI_SDK_DIR/bin/clang" ]; then
	echo "ERROR: wasi-sdk clang not found at $WASI_SDK_DIR/bin/clang" >&2
	echo "       Run: make -C toolchain/c wasi-sdk" >&2
	exit 1
fi

echo "== codex repo: $CODEX_REPO =="
echo "== wasi-sdk:   $WASI_SDK_DIR =="
echo "== dest:       $DEST_DIR =="

WASI_SDK_DIR="$(cd "$WASI_SDK_DIR" && pwd)" INSTALL=0 "$BUILD_SCRIPT"

OUT="$CODEX_REPO/target/wasm32-wasip1/release/codex-exec.opt.wasm"
if [ ! -f "$OUT" ]; then
	echo "ERROR: expected build output missing: $OUT" >&2
	exit 1
fi

mkdir -p "$DEST_DIR"
cp "$OUT" "$DEST_DIR/codex-exec"
cp "$OUT" "$DEST_DIR/codex"

echo "== installed $(wc -c < "$OUT") bytes to $DEST_DIR/{codex-exec,codex} =="
