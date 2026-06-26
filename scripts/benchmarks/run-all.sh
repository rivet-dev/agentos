#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

cargo build -p agentos-sidecar --release
cargo build -p native-baseline --release --manifest-path /home/nathan/.herdr/workspaces/secure-exec/fuzz-perf/Cargo.toml

export AGENTOS_SIDECAR_BIN="${AGENTOS_SIDECAR_BIN:-$REPO_ROOT/target/release/agentos-sidecar}"
export NATIVE_BASELINE_BIN="${NATIVE_BASELINE_BIN:-/home/nathan/.herdr/workspaces/secure-exec/fuzz-perf/target/release/native-baseline}"
export NODE_OPTIONS="${NODE_OPTIONS:---expose-gc}"

pnpm exec tsx scripts/benchmarks/run-all.ts
