#!/bin/bash
# Hermetically assemble and build the pinned Node runtime WASM module.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(cd "$CRATE_DIR/../.." && pwd)"
VENDOR_DIR="$CRATE_DIR/vendor"
PATCH_DIR="$CRATE_DIR/patches"

: "${AGENTOS_WASI_SDK_ROOT:?AGENTOS_WASI_SDK_ROOT must name the pinned wasi-sdk}"
: "${AGENTOS_WASI_SYSROOT:?AGENTOS_WASI_SYSROOT must name the threaded AgentOS sysroot}"
: "${AGENTOS_OPENSSL_ROOT:?AGENTOS_OPENSSL_ROOT must name the threaded OpenSSL install}"

BUILD_ROOT="${AGENTOS_NODE_RUNTIME_BUILD_DIR:-$REPO_ROOT/target/node-runtime-wasm}"
SOURCE_DIR="$BUILD_ROOT/source"
CMAKE_BUILD_DIR="$BUILD_ROOT/cmake"
OUTPUT_WASM="$BUILD_ROOT/node-runtime.wasm"
LINKED_WASM="$BUILD_ROOT/node-runtime.linked.wasm"
OUTPUT_MANIFEST="$BUILD_ROOT/node-runtime-wasm-abi.json"
OUTPUT_BUILD_MANIFEST="$BUILD_ROOT/node-runtime-wasm-build.json"
JOBS="${AGENTOS_BUILD_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}"

# Node v24.15.0 commit time, shared with build/openssl.json. Keep every
# generated archive and compiler invocation independent of the caller's locale,
# timezone, and wall clock so two clean offline builds are byte-identical.
export LC_ALL=C
export TZ=UTC
export SOURCE_DATE_EPOCH=1775854510
export ZERO_AR_DATE=1

case "$BUILD_ROOT" in
  ""|"/"|"$REPO_ROOT")
    echo "refusing unsafe AGENTOS_NODE_RUNTIME_BUILD_DIR: $BUILD_ROOT" >&2
    exit 2
    ;;
esac

for required in \
  "$AGENTOS_WASI_SDK_ROOT/bin/clang" \
  "$AGENTOS_WASI_SYSROOT/lib/wasm32-wasi-threads/libc.a" \
  "$AGENTOS_WASI_SYSROOT/lib/wasm32-wasi-threads/libsetjmp.a" \
  "$AGENTOS_OPENSSL_ROOT/lib/libcrypto.a" \
  "$AGENTOS_OPENSSL_ROOT/lib/libssl.a" \
  "$AGENTOS_OPENSSL_ROOT/manifest.json" \
  "$VENDOR_DIR/manifest.json"; do
  if [ ! -e "$required" ]; then
    echo "required pinned build input is missing: $required" >&2
    exit 2
  fi
done

rm -rf "$SOURCE_DIR" "$CMAKE_BUILD_DIR"
mkdir -p "$SOURCE_DIR/deps" "$SOURCE_DIR/napi" "$CMAKE_BUILD_DIR"
cp -a "$VENDOR_DIR/edgejs/." "$SOURCE_DIR/"

for patch in "$PATCH_DIR"/edgejs/*.patch; do
  git -C "$SOURCE_DIR" apply "$patch"
done

cp -a "$VENDOR_DIR/napi/include" "$SOURCE_DIR/napi/include"
for patch in "$PATCH_DIR"/napi/*.patch; do
  git -C "$SOURCE_DIR/napi" apply "$patch"
done

for dependency in cares icu-small ncrypto simdjson; do
  cp -a "$VENDOR_DIR/node/deps/$dependency" "$SOURCE_DIR/deps/$dependency"
done
for dependency in ada brotli llhttp merve nbytes nghttp2 simdutf zlib zstd; do
  cp -a "$REPO_ROOT/crates/node-stdlib/vendor/native/$dependency" \
    "$SOURCE_DIR/deps/$dependency"
done

cp -a "$VENDOR_DIR/libuv" "$SOURCE_DIR/deps/libuv-wasix"
for patch in "$PATCH_DIR"/libuv/*.patch; do
  git -C "$SOURCE_DIR/deps/libuv-wasix" apply "$patch"
done

AGENTOS_WASI_SDK_ROOT="$AGENTOS_WASI_SDK_ROOT" \
AGENTOS_WASI_SYSROOT="$AGENTOS_WASI_SYSROOT" \
cmake -S "$SOURCE_DIR" -B "$CMAKE_BUILD_DIR" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_TOOLCHAIN_FILE="$CRATE_DIR/cmake/agentos-toolchain.cmake" \
  -DEDGE_AGENTOS_RUNTIME=ON \
  -DEDGE_BUILD_CLI=ON \
  -DEDGE_BUILD_NAPI_TESTS=OFF \
  -DEDGE_NAPI_PROVIDER=imports \
  -DEDGE_OPENSSL_WASIX_ROOT="$AGENTOS_OPENSSL_ROOT" \
  -DEDGE_QUICKJS_WEBASSEMBLY=OFF

cmake --build "$CMAKE_BUILD_DIR" --target edge -j"$JOBS"
cmake -E copy "$CMAKE_BUILD_DIR/edge" "$LINKED_WASM"

# wasi-libc still spells its standard POSIX transport as the historical WASI
# module. Normalize those declarations into the same versioned syscall object
# as the AgentOS libc additions, and reject every unexpected import module.
cargo run --quiet -p agentos-node-runtime-wasm --bin normalize-runtime-imports -- \
  "$LINKED_WASM" "$OUTPUT_WASM"

cargo run --quiet -p agentos-node-runtime-wasm --bin generate-abi-manifest -- \
  "$OUTPUT_WASM" "$OUTPUT_MANIFEST"

cargo run --quiet -p agentos-node-runtime-wasm --bin generate-build-manifest -- \
  "$OUTPUT_WASM" \
  "$OUTPUT_MANIFEST" \
  "$VENDOR_DIR/manifest.json" \
  "$PATCH_DIR" \
  "$SOURCE_DIR" \
  "$AGENTOS_WASI_SYSROOT" \
  "$AGENTOS_OPENSSL_ROOT" \
  "$AGENTOS_WASI_SDK_ROOT" \
  "$OUTPUT_BUILD_MANIFEST"

AGENTOS_NODE_API_SYSROOT="$AGENTOS_WASI_SYSROOT" \
  node "$REPO_ROOT/scripts/generate-node-api-v1-v10-inventory.mjs" --check
node "$REPO_ROOT/scripts/generate-node-runtime-wasm-abi-inventories.mjs" --check
node "$REPO_ROOT/scripts/generate-node-runtime-wasm-engine-contract.mjs" --check
node "$REPO_ROOT/scripts/generate-node-runtime-wasm-posix-contract.mjs" --check
node "$REPO_ROOT/scripts/generate-wasm-posix-host-table.mjs" --check
node "$REPO_ROOT/scripts/generate-node-runtime-wasm-performance.mjs" --check
node "$REPO_ROOT/scripts/check-node-runtime-wasm-forbidden.mjs" \
  --abi "$OUTPUT_MANIFEST" \
  --build "$OUTPUT_BUILD_MANIFEST"

echo "Node runtime WASM: $OUTPUT_WASM"
echo "ABI manifest: $OUTPUT_MANIFEST"
echo "Build manifest: $OUTPUT_BUILD_MANIFEST"
sha256sum "$OUTPUT_WASM" "$OUTPUT_MANIFEST" "$OUTPUT_BUILD_MANIFEST"
