#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

RESULTS_DIR="$SCRIPT_DIR/results"
mkdir -p "$RESULTS_DIR"

BENCH_ONLY="${BENCH_ONLY:-}"
LANES=(
  "coldstart-sleep"
  "memory-sleep"
  "memory-pi-session"
  "session"
  "actor-session"
)

should_run() {
  local name="$1"
  [[ -z "$BENCH_ONLY" || "$BENCH_ONLY" == "$name" ]]
}

should_build() {
  [[ -z "$BENCH_ONLY" ]] && return 0
  local lane
  for lane in "${LANES[@]}"; do
    [[ "$BENCH_ONLY" == "$lane" ]] && return 0
  done
  return 1
}

run() {
  local name="$1"
  shift
  if ! should_run "$name"; then
    echo "=== Skipping $name (BENCH_ONLY=$BENCH_ONLY) ===" >&2
    return
  fi
  echo "" >&2
  echo "=== Running $name ===" >&2
  pnpm exec tsx "$@" \
    1> "$RESULTS_DIR/${name}.json" \
    2> >(tee "$RESULTS_DIR/${name}.log" >&2)
}

run_to_file() {
  local name="$1"
  shift
  if ! should_run "$name"; then
    echo "=== Skipping $name (BENCH_ONLY=$BENCH_ONLY) ===" >&2
    return
  fi
  echo "" >&2
  echo "=== Running $name ===" >&2
  pnpm exec tsx "$@" --output="$RESULTS_DIR/${name}.json" \
    2>&1 | tee "$RESULTS_DIR/${name}.log" >&2
}

if should_build; then
  echo "=== Building benchmark TypeScript dependencies ===" >&2
  pnpm --dir packages/core build >&2

  # Benchmarks must run against an optimized sidecar. The debug build is several
  # times slower and inflates cold-start and memory numbers.
  echo "=== Building release sidecar ===" >&2
  cargo build --release -p agentos-sidecar >&2
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    export AGENTOS_SIDECAR_BIN="$CARGO_TARGET_DIR/release/agentos-sidecar"
  else
    export AGENTOS_SIDECAR_BIN="$REPO_ROOT/target/release/agentos-sidecar"
  fi
  echo "Using sidecar: $AGENTOS_SIDECAR_BIN" >&2

  if should_run "actor-session"; then
    echo "=== Building actor-session benchmark dependencies ===" >&2
    pnpm --filter @agentos-software/claude-code build >&2
    pnpm --filter @agentos-software/pi build >&2
    pnpm --filter @agentos-software/codex-cli build >&2
    pnpm --filter @agentos-software/codex build >&2
    if ! pnpm --filter @agentos-software/opencode exec bun --version >/dev/null 2>&1; then
      echo "=== Materializing Bun for the OpenCode package build ===" >&2
      pnpm --filter @agentos-software/opencode exec node node_modules/bun/install.js >&2
    fi
    pnpm --filter @agentos-software/opencode build >&2
    pnpm --dir packages/agentos build >&2
    cargo build --release -p agentos-actor-plugin >&2
    echo "Using release actor plugin from the Cargo target directory" >&2
  fi
else
  echo "=== No matching benchmark lanes for BENCH_ONLY=$BENCH_ONLY ===" >&2
fi

# Cold start - minimal "sleep" VM (idle Node.js process). This is the workload
# cited on the product cold-start chart. 2000 iterations keeps p95/p99 meaningful.
run "coldstart-sleep" \
  scripts/benchmarks/coldstart.bench.ts --workload=sleep --iterations=2000

# Memory - simple shell workload.
run "memory-sleep" \
  --expose-gc scripts/benchmarks/memory.bench.ts --workload=sleep --count=20

# Memory - full Pi agent session.
run "memory-pi-session" \
  --expose-gc scripts/benchmarks/memory.bench.ts --workload=pi-session --count=10

# Session-creation VM-tax benchmark (deterministic, llmock-backed).
# Set BENCH_GATE=1 to fail on regression; set BENCH_UPDATE_BASELINE=1 to refresh
# scripts/benchmarks/baseline.json.
run "session" \
  scripts/benchmarks/session.bench.ts --iterations=5 \
    ${BENCH_GATE:+--gate} ${BENCH_UPDATE_BASELINE:+--update-baseline}

# End-to-end actor session startup + first message (local llmock provider).
run_to_file "actor-session" \
  scripts/benchmarks/actor-session.bench.ts \
    --iterations="${BENCH_ITERATIONS:-5}" --warmup="${BENCH_WARMUP:-1}"

echo "" >&2
echo "=== Done. Results in $RESULTS_DIR ===" >&2
