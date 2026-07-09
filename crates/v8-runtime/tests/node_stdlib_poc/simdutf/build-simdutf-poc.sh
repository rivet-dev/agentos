#!/usr/bin/env bash
# Build the stage-2.5 POC wasm: upstream simdutf compiled to wasm32-wasip1
# against the agentOS patched sysroot, exported behind a flat C ABI
# (wrapper.cpp) as a wasm reactor for instantiation inside the V8 isolate.
#
# The C toolchain (wasi-sdk + patched sysroot) lives on the reg-tests branch,
# not on main, so its location is taken as input:
#   AGENTOS_C_TOOLCHAIN  root containing vendor/wasi-sdk and sysroot/
#                        (default: the reg-tests workspace checkout)
#
# Output: build/simdutf-poc.wasm next to this script. The cargo tests pick it
# up automatically (or via AGENTOS_SIMDUTF_POC_WASM) and skip the wasm-backed
# assertions when it is absent.
set -euo pipefail

SIMDUTF_VERSION="v9.0.0"
SIMDUTF_URL="https://github.com/simdutf/simdutf/releases/download/${SIMDUTF_VERSION}/singleheader.zip"
SIMDUTF_SHA256="c47c68cd51025ec66509bc36215b4c4f1f0f0a98129139ee55c541531b652526"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
TOOLCHAIN="${AGENTOS_C_TOOLCHAIN:-/home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c}"
CLANGXX="$TOOLCHAIN/vendor/wasi-sdk/bin/clang++"
SYSROOT="$TOOLCHAIN/sysroot"

if [[ ! -x "$CLANGXX" ]]; then
  echo "error: wasi-sdk clang++ not found at $CLANGXX (set AGENTOS_C_TOOLCHAIN)" >&2
  exit 1
fi
if [[ ! -f "$SYSROOT/lib/wasm32-wasi/libc++.a" ]]; then
  echo "error: patched sysroot with libc++ not found at $SYSROOT" >&2
  exit 1
fi

mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

if [[ ! -f simdutf.cpp || ! -f simdutf.h ]]; then
  echo "fetching simdutf $SIMDUTF_VERSION singleheader"
  curl -fsSL -o singleheader.zip "$SIMDUTF_URL"
  echo "$SIMDUTF_SHA256  singleheader.zip" | sha256sum -c -
  unzip -o -q singleheader.zip simdutf.cpp simdutf.h
fi

echo "compiling simdutf-poc.wasm (simdutf $SIMDUTF_VERSION, sysroot $SYSROOT)"
"$CLANGXX" \
  --target=wasm32-wasip1 \
  --sysroot="$SYSROOT" \
  -isystem "$SYSROOT/include/wasm32-wasi/c++/v1" \
  -isystem "$SYSROOT/include/wasm32-wasi" \
  -I "$BUILD_DIR" \
  -D_WASI_EMULATED_PTHREAD \
  -O2 -fno-exceptions \
  -mexec-model=reactor \
  -lwasi-emulated-pthread \
  -Wl,--export=poc_alloc \
  -Wl,--export=poc_free \
  -Wl,--export=poc_is_utf8 \
  -Wl,--export=poc_is_ascii \
  -Wl,--export=poc_utf8_len_from_utf16le \
  -o simdutf-poc.wasm \
  "$SCRIPT_DIR/wrapper.cpp" simdutf.cpp

WASM_OPT="$TOOLCHAIN/vendor/wasi-sdk/bin/wasm-opt"
if [[ -x "$WASM_OPT" ]]; then
  "$WASM_OPT" -O3 --strip-debug simdutf-poc.wasm -o simdutf-poc.wasm
fi

ls -la simdutf-poc.wasm
