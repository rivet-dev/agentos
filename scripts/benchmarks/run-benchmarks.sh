#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

echo "=== Building TypeScript ===" >&2
pnpm build >&2

# Benchmarks must run against an OPTIMIZED sidecar — the debug build is several
# times slower and inflates cold-start and memory numbers. Build the release
# binary and point the SDK at it via AGENT_OS_SIDECAR_BIN.
echo "=== Building release sidecar ===" >&2
cargo build --release -p agent-os-sidecar >&2
export AGENT_OS_SIDECAR_BIN="$REPO_ROOT/target/release/agent-os-sidecar"
echo "Using sidecar: $AGENT_OS_SIDECAR_BIN" >&2

RESULTS_DIR="$SCRIPT_DIR/results"
mkdir -p "$RESULTS_DIR"

run() {
  local name="$1"
  shift
  echo "" >&2
  echo "=== Running $name ===" >&2
  pnpm exec tsx "$@" \
    1> "$RESULTS_DIR/${name}.json" \
    2> >(tee "$RESULTS_DIR/${name}.log" >&2)
}

# Cold start — minimal "sleep" VM (idle Node.js process). This is the workload
# cited on the marketing cold-start chart. 2000 iterations so the reported p95
# (needs ~200+ samples) and p99 (needs ~1000+) are statistically meaningful, not
# just the slowest one or two runs.
run "coldstart-sleep" \
  scripts/benchmarks/coldstart.bench.ts --workload=sleep --iterations=2000

# Memory — simple shell workload (the "Simple shell command" marketing row).
run "memory-sleep" \
  --expose-gc scripts/benchmarks/memory.bench.ts --workload=sleep --count=20

# Memory — full Pi agent session (the "Full coding agent" marketing row).
run "memory-pi-session" \
  --expose-gc scripts/benchmarks/memory.bench.ts --workload=pi-session --count=10

# Session-creation VM-tax benchmark (deterministic, llmock-backed).
# Compares the agentOS VM path vs the bare-node pi-SDK equivalent and gates the
# deterministic metrics against scripts/benchmarks/baseline.json.
# Set BENCH_GATE=1 to fail the run on a regression (CI); set BENCH_UPDATE_BASELINE=1
# to refresh the committed baseline (do this on a clean checkout, review in PR).
echo "" >&2
echo "=== Running session ===" >&2
pnpm exec tsx scripts/benchmarks/session.bench.ts --iterations=5 \
  ${BENCH_GATE:+--gate} ${BENCH_UPDATE_BASELINE:+--update-baseline} \
  1> "$RESULTS_DIR/session.json" \
  2> >(tee "$RESULTS_DIR/session.log" >&2)

echo "" >&2
echo "=== Done. Results in $RESULTS_DIR ===" >&2
echo "Update website/src/data/bench.ts inputs from these JSON files." >&2
