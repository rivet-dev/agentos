#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(cd "$CRATE_DIR/../.." && pwd)"

: "${AGENTOS_WASI_SDK_ROOT:?AGENTOS_WASI_SDK_ROOT must name the pinned wasi-sdk}"
: "${AGENTOS_WASI_SYSROOT:?AGENTOS_WASI_SYSROOT must name the threaded AgentOS sysroot}"

MODE="build"
if [ "${1:-}" = "--check" ]; then
  MODE="check"
  CHECK_DIR="$(mktemp -d)"
  trap 'rm -rf "$CHECK_DIR"' EXIT
  OUTPUT="$CHECK_DIR/pthread-runtime.wasm"
else
  OUTPUT="${1:-$REPO_ROOT/target/node-runtime-wasm/pthread-runtime.wasm}"
fi
RAW_OUTPUT="${OUTPUT%.wasm}.linked.wasm"
if [ "$MODE" = "build" ]; then
  trap 'rm -f "$RAW_OUTPUT"' EXIT
fi
mkdir -p "$(dirname "$OUTPUT")"

export LC_ALL=C
export TZ=UTC
export SOURCE_DATE_EPOCH=1775854510
export ZERO_AR_DATE=1

"$AGENTOS_WASI_SDK_ROOT/bin/clang" \
  --target=wasm32-wasi-threads \
  --sysroot="$AGENTOS_WASI_SYSROOT" \
  -O2 \
  -pthread \
  -matomics \
  -mbulk-memory \
  -fwasm-exceptions \
  -mexec-model=reactor \
  -ffile-prefix-map="$REPO_ROOT=." \
  -Wl,--import-memory \
  -Wl,--shared-memory \
  -Wl,--initial-memory=67108864 \
  -Wl,--max-memory=268435456 \
  -Wl,--export-table \
  -Wl,--import-undefined \
  -lsetjmp \
  -o "$RAW_OUTPUT" \
  "$CRATE_DIR/probes/pthread-runtime.c"

cargo run --quiet -p agentos-node-runtime-wasm --bin normalize-runtime-imports -- \
  "$RAW_OUTPUT" "$OUTPUT"
sha256sum "$OUTPUT"

if [ "$MODE" = "check" ]; then
  ARTIFACT="$CRATE_DIR/artifacts/pthread-runtime.wasm"
  MANIFEST="$CRATE_DIR/probes/pthread-manifest.json"
  cmp "$OUTPUT" "$ARTIFACT"
  node - "$MANIFEST" "$CRATE_DIR" "$AGENTOS_WASI_SYSROOT" "$AGENTOS_WASI_SDK_ROOT" <<'NODE'
const { createHash } = require('node:crypto');
const { readFileSync, statSync } = require('node:fs');
const { join } = require('node:path');
const [manifestPath, crateDir, sysroot, wasiSdk] = process.argv.slice(2);
const manifest = JSON.parse(readFileSync(manifestPath));
const sha = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');
const checks = [
  [join(crateDir, manifest.source.path), manifest.source.sha256, 'source'],
  [join(crateDir, manifest.artifact.path), manifest.artifact.sha256, 'artifact'],
  [join(sysroot, 'lib/wasm32-wasi-threads/libc.a'), manifest.sysroot.libcSha256, 'libc'],
  [join(sysroot, 'lib/wasm32-wasi-threads/libsetjmp.a'), manifest.sysroot.libsetjmpSha256, 'libsetjmp'],
  [join(wasiSdk, 'bin/clang'), manifest.toolchain.clangSha256, 'clang'],
];
for (const [path, expected, label] of checks) {
  const actual = sha(path);
  if (actual !== expected) throw new Error(`${label} hash drifted: ${actual}`);
}
const artifact = join(crateDir, manifest.artifact.path);
if (statSync(artifact).size !== manifest.artifact.byteLength) {
  throw new Error(`pthread artifact size drifted: ${statSync(artifact).size}`);
}
const module = new WebAssembly.Module(readFileSync(artifact));
const actualImports = WebAssembly.Module.imports(module).map(({ module, name, kind }) => ({ module, name, kind }));
const expectedImports = manifest.imports.map(({ module, name }) => ({ module, name, kind: name === 'memory' ? 'memory' : 'function' }));
if (JSON.stringify(actualImports) !== JSON.stringify(expectedImports)) {
  throw new Error(`pthread import ABI drifted: ${JSON.stringify(actualImports)}`);
}
const actualExports = WebAssembly.Module.exports(module).map(({ name }) => name);
if (JSON.stringify(actualExports) !== JSON.stringify(manifest.exports)) {
  throw new Error(`pthread export ABI drifted: ${JSON.stringify(actualExports)}`);
}
NODE
  echo "pthread runtime probe: reproducible artifact, toolchain, sysroot, and ABI verified"
fi
