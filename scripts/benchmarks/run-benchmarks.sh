#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

echo "=== Building benchmark TypeScript dependencies ===" >&2
pnpm --dir packages/core build >&2

# Benchmarks must run against an OPTIMIZED sidecar — the debug build is several
# times slower and inflates cold-start and memory numbers. Build the release
# binary and point the SDK at it via AGENTOS_SIDECAR_BIN.
echo "=== Building release sidecar ===" >&2
cargo build --release -p agentos-sidecar >&2
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  export AGENTOS_SIDECAR_BIN="$CARGO_TARGET_DIR/release/agentos-sidecar"
else
  export AGENTOS_SIDECAR_BIN="$REPO_ROOT/target/release/agentos-sidecar"
fi
echo "Using sidecar: $AGENTOS_SIDECAR_BIN" >&2

RESULTS_DIR="$SCRIPT_DIR/results"
mkdir -p "$RESULTS_DIR"

BENCH_ONLY="${BENCH_ONLY:-}"

should_run() {
  local name="$1"
  [[ -z "$BENCH_ONLY" || "$BENCH_ONLY" == "$name" ]]
}

resolve_secure_exec_root() {
  if [[ -n "${SECURE_EXEC_ROOT:-}" ]]; then
    echo "$SECURE_EXEC_ROOT"
  elif [[ -f "$REPO_ROOT/../secure-exec/crates/native-baseline/Cargo.toml" ]]; then
    echo "$REPO_ROOT/../secure-exec"
  elif [[ -f "$REPO_ROOT/../../secure-exec/fuzz-perf/crates/native-baseline/Cargo.toml" ]]; then
    echo "$REPO_ROOT/../../secure-exec/fuzz-perf"
  else
    echo "Unable to find secure-exec checkout with crates/native-baseline. Set SECURE_EXEC_ROOT=/path/to/secure-exec." >&2
    exit 1
  fi
}

build_native_baseline() {
  local secure_exec_root
  secure_exec_root="$(resolve_secure_exec_root)"
  echo "" >&2
  echo "=== Building native-baseline ===" >&2
  cargo build -p native-baseline --release --manifest-path "$secure_exec_root/Cargo.toml" >&2
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    export NATIVE_BASELINE_BIN="${NATIVE_BASELINE_BIN:-$CARGO_TARGET_DIR/release/native-baseline}"
  else
    export NATIVE_BASELINE_BIN="${NATIVE_BASELINE_BIN:-$secure_exec_root/target/release/native-baseline}"
  fi
  echo "Using native baseline: $NATIVE_BASELINE_BIN" >&2
}

build_native_baseline_wasm() {
  local secure_exec_root
  secure_exec_root="$(resolve_secure_exec_root)"
  if command -v rustup >/dev/null 2>&1 && ! rustup target list --installed | grep -qx "wasm32-wasip1"; then
    echo "=== Skipping vm-wasm lane: Rust target wasm32-wasip1 is not installed ===" >&2
    echo "Install it with: rustup target add wasm32-wasip1" >&2
    export NATIVE_BASELINE_WASM=""
    return
  fi
  echo "" >&2
  echo "=== Building native-baseline wasm32-wasip1 ===" >&2
  cargo build --release --target wasm32-wasip1 -p native-baseline --manifest-path "$secure_exec_root/Cargo.toml" >&2
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    export NATIVE_BASELINE_WASM="${NATIVE_BASELINE_WASM:-$CARGO_TARGET_DIR/wasm32-wasip1/release/native-baseline.wasm}"
  else
    export NATIVE_BASELINE_WASM="${NATIVE_BASELINE_WASM:-$secure_exec_root/target/wasm32-wasip1/release/native-baseline.wasm}"
  fi
  echo "Using wasm native baseline: $NATIVE_BASELINE_WASM" >&2
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

# Cold start — minimal "sleep" VM (idle Node.js process). This is the workload
# cited on the marketing cold-start chart. 2000 iterations so the reported p95
# (needs ~200+ samples) and p99 (needs ~1000+) are statistically meaningful, not
# just the slowest one or two runs.
run "coldstart-sleep" \
  scripts/benchmarks/coldstart.bench.ts --workload=sleep --iterations=2000

# WASM shell echo cold/warm command floor. This is the legacy standalone echo
# benchmark, kept in the standard suite so every *.bench.ts has a suite entry.
run "echo-cold-warm" \
  scripts/benchmarks/echo.bench.ts

# Memory — simple shell workload (the "Simple shell command" marketing row).
run "memory-sleep" \
  --expose-gc scripts/benchmarks/memory.bench.ts --workload=sleep --count=20

# Memory — full Pi agent session (the "Full coding agent" marketing row).
run "memory-pi-session" \
  --expose-gc scripts/benchmarks/memory.bench.ts --workload=pi-session --count=10

# VM startup + serial `ls`, from the VM-ls investigation thread. This keeps the
# benchmark in the standard suite instead of leaving it as an ad-hoc diagnostic.
run "ls-serial" \
  scripts/benchmarks/ls.bench.ts \
    --iterations="${BENCH_LS_ITERATIONS:-5}" \
    --warmup="${BENCH_LS_WARMUP:-1}" \
    --serial-runs="${BENCH_LS_SERIAL_RUNS:-5}" \
    --file-counts="${BENCH_LS_FILE_COUNTS:-0,100}" \
    ${BENCH_LS_WASM_WARMUP_DEBUG:+--wasm-warmup-debug}

# Focused WASI/coreutils `ls` scaling. Prepares fixtures once and reuses one VM
# so the timed loop isolates command/directory-size cost.
run "wasi-ls-scaling" \
  scripts/benchmarks/wasi-ls-scaling.bench.ts \
    --iterations="${BENCH_WASI_LS_ITERATIONS:-5}" \
    --warmup="${BENCH_WASI_LS_WARMUP:-1}" \
    --serial-runs="${BENCH_WASI_LS_SERIAL_RUNS:-3}" \
    --file-counts="${BENCH_WASI_LS_FILE_COUNTS:-0,1,32,100,1000}" \
    --ls-variants="${BENCH_WASI_LS_VARIANTS:-one}" \
    ${BENCH_WASI_LS_WASM_WARMUP_DEBUG:+--wasm-warmup-debug} \
    ${BENCH_WASI_LS_SYSCALL_COUNTERS:+--wasi-syscall-counters}

# WASI/coreutils `ls` attribution lane. This promotes the granular Codex-thread
# follow-up into the standard suite: syscall counters plus representative empty,
# 100-file, and 1000-file directories.
run "wasi-ls-scaling-counters" \
  scripts/benchmarks/wasi-ls-scaling.bench.ts \
    --iterations="${BENCH_WASI_LS_COUNTER_ITERATIONS:-3}" \
    --warmup="${BENCH_WASI_LS_COUNTER_WARMUP:-1}" \
    --serial-runs="${BENCH_WASI_LS_COUNTER_SERIAL_RUNS:-3}" \
    --file-counts="${BENCH_WASI_LS_COUNTER_FILE_COUNTS:-0,100,1000}" \
    --ls-variants="${BENCH_WASI_LS_COUNTER_VARIANTS:-one,fast-no-decor}" \
    --wasi-syscall-counters \
    ${BENCH_WASI_LS_COUNTER_WASM_WARMUP_DEBUG:+--wasm-warmup-debug}

# Pure readdir scaling, with fixture setup outside the timed loop. This is the
# focused directory-listing benchmark used to validate readdir optimizations.
run "readdir-scaling" \
  scripts/benchmarks/readdir.bench.ts \
    --iterations="${BENCH_READDIR_ITERATIONS:-10}" \
    --warmup="${BENCH_READDIR_WARMUP:-2}" \
    --entry-counts="${BENCH_READDIR_ENTRY_COUNTS:-0,1,32,100,1000}" \
    --modes="${BENCH_READDIR_MODES:-plain,withFileTypes}" \
    --fixtures="${BENCH_READDIR_FIXTURES:-vm-shadow}" \
    --workloads="${BENCH_READDIR_WORKLOADS:-pure}" \
    ${BENCH_READDIR_PREFLIGHT_OPS:+--preflight-ops="${BENCH_READDIR_PREFLIGHT_OPS}"} \
    ${BENCH_READDIR_PREFLIGHT_COUNTS:+--preflight-counts="${BENCH_READDIR_PREFLIGHT_COUNTS}"} \
    ${BENCH_READDIR_INCLUDE_READDIR:+--include-readdir="${BENCH_READDIR_INCLUDE_READDIR}"} \
    ${BENCH_READDIR_PROBE_TARGETS:+--probe-targets="${BENCH_READDIR_PROBE_TARGETS}"}

# Readdir guard/probe lane. This keeps the historical guarded readdir shape and
# the repeated sync-preflight diagnosis in the normal suite output.
run "readdir-probe" \
  scripts/benchmarks/readdir.bench.ts \
    --iterations="${BENCH_READDIR_PROBE_ITERATIONS:-5}" \
    --warmup="${BENCH_READDIR_PROBE_WARMUP:-1}" \
    --entry-counts="${BENCH_READDIR_PROBE_ENTRY_COUNTS:-32}" \
    --modes="${BENCH_READDIR_PROBE_MODES:-plain}" \
    --fixtures="${BENCH_READDIR_PROBE_FIXTURES:-vm-shadow,native-host-dir}" \
    --workloads=probe \
    --preflight-ops="${BENCH_READDIR_PROBE_PREFLIGHT_OPS:-none,existsSync,statSync}" \
    --preflight-counts="${BENCH_READDIR_PROBE_PREFLIGHT_COUNTS:-0,32,33}" \
    --include-readdir="${BENCH_READDIR_PROBE_INCLUDE_READDIR:-both}" \
    --probe-targets="${BENCH_READDIR_PROBE_TARGETS:-dir-plus-children}"

# Focused synchronous filesystem operation floor. Separates user-facing Node API
# calls from the estimated bridge call count for small sync filesystem bundles.
run "fs-sync-ops" \
  scripts/benchmarks/fs-sync-ops.bench.ts \
    --iterations="${BENCH_FS_SYNC_ITERATIONS:-10}" \
    --warmup="${BENCH_FS_SYNC_WARMUP:-2}" \
    --ops="${BENCH_FS_SYNC_OPS:-existsSync,statSync,openClose,mkdirRmdir,smallWrite,readFileSync,renameFile}" \
    --call-counts="${BENCH_FS_SYNC_CALL_COUNTS:-1,8,32}" \
    --fixtures="${BENCH_FS_SYNC_FIXTURES:-vm-shadow}" \
    --payload-bytes="${BENCH_FS_SYNC_PAYLOAD_BYTES:-8}" \
    ${BENCH_FS_SYNC_RPC_LATENCY:+--sync-rpc-latency} \
    ${BENCH_FS_SYNC_PHASES:+--fs-sync-phases}

# Phase-enabled sync filesystem attribution. Separate lane so the default
# user-facing op floor stays broad while this row captures bridge/sidecar phases.
run "fs-sync-ops-phases" \
  scripts/benchmarks/fs-sync-ops.bench.ts \
    --iterations="${BENCH_FS_SYNC_PHASE_ITERATIONS:-5}" \
    --warmup="${BENCH_FS_SYNC_PHASE_WARMUP:-1}" \
    --ops="${BENCH_FS_SYNC_PHASE_OPS:-existsSync,statSync,readFileSync}" \
    --call-counts="${BENCH_FS_SYNC_PHASE_CALL_COUNTS:-8,32}" \
    --fixtures="${BENCH_FS_SYNC_PHASE_FIXTURES:-vm-shadow}" \
    --payload-bytes="${BENCH_FS_SYNC_PHASE_PAYLOAD_BYTES:-8}" \
    --sync-rpc-latency \
    --fs-sync-phases

# Pure sync bridge floor. Calls a benchmark-only no-op bridge RPC that returns
# before filesystem/VFS dispatch, isolating bridge/routing/serialization cost.
run "sync-bridge-floor" \
  scripts/benchmarks/sync-bridge-floor.bench.ts \
    --iterations="${BENCH_SYNC_BRIDGE_ITERATIONS:-10}" \
    --warmup="${BENCH_SYNC_BRIDGE_WARMUP:-2}" \
    --call-counts="${BENCH_SYNC_BRIDGE_CALL_COUNTS:-1,8,32}" \
    --payload-bytes="${BENCH_SYNC_BRIDGE_PAYLOAD_BYTES:-0}" \
    ${BENCH_SYNC_BRIDGE_RPC_LATENCY:+--sync-rpc-latency} \
    ${BENCH_SYNC_BRIDGE_PHASES:+--bridge-phases}

# Phase-enabled pure bridge attribution. This is the standard guard for sync-RPC
# request/response servicing optimizations.
run "sync-bridge-floor-phases" \
  scripts/benchmarks/sync-bridge-floor.bench.ts \
    --iterations="${BENCH_SYNC_BRIDGE_PHASE_ITERATIONS:-5}" \
    --warmup="${BENCH_SYNC_BRIDGE_PHASE_WARMUP:-1}" \
    --call-counts="${BENCH_SYNC_BRIDGE_PHASE_CALL_COUNTS:-1,8,32}" \
    --payload-bytes="${BENCH_SYNC_BRIDGE_PHASE_PAYLOAD_BYTES:-0}" \
    --sync-rpc-latency \
    --bridge-phases

# Large-args sync bridge lane. Same no-op bridge RPC, but with a 64KiB payload so
# the per-call argument-buffer clone (bridge.rs:1978) is material. This is the
# standing regression guard for the args-serialization buffer reuse.
run "sync-bridge-floor-bigargs" \
  scripts/benchmarks/sync-bridge-floor.bench.ts \
    --iterations="${BENCH_SYNC_BRIDGE_BIGARGS_ITERATIONS:-10}" \
    --warmup="${BENCH_SYNC_BRIDGE_BIGARGS_WARMUP:-2}" \
    --call-counts="${BENCH_SYNC_BRIDGE_BIGARGS_CALL_COUNTS:-1,8,32}" \
    --payload-bytes="${BENCH_SYNC_BRIDGE_BIGARGS_PAYLOAD_BYTES:-65536}"

# Focused DNS lookup floor. Splits the broad dns rows into warm, repeated,
# concurrent, and fresh-process lookup shapes.
run "dns-lookup-floor" \
  scripts/benchmarks/dns-lookup-floor.bench.ts \
    --iterations="${BENCH_DNS_LOOKUP_ITERATIONS:-10}" \
    --warmup="${BENCH_DNS_LOOKUP_WARMUP:-2}" \
    --rows="${BENCH_DNS_LOOKUP_ROWS:-single_localhost,sequential_same_2,sequential_same_8,sequential_same_32,concurrent_same_4,concurrent_same_16,cold_process_single}"

# Focused TCP loopback benchmark. Separates payload size, write count, and
# client concurrency for the broad net/tcp_* rows.
run "net-tcp-event-floor" \
  scripts/benchmarks/net-tcp-event-floor.bench.ts \
    --iterations="${BENCH_NET_TCP_ITERATIONS:-20}" \
    --warmup="${BENCH_NET_TCP_WARMUP:-5}" \
    ${BENCH_NET_TCP_ROWS:+--rows="${BENCH_NET_TCP_ROWS}"} \
    ${BENCH_NET_TCP_POLL_DELAY_MS:+--net-poll-delay-ms="${BENCH_NET_TCP_POLL_DELAY_MS}"} \
    ${BENCH_NET_TCP_TRACE:+--net-bridge-trace}

# Counter-backed TCP cadence attribution. Kept as a separate standard lane so
# trace fields are always available when comparing network event-loop changes.
# This is the standard home for the later TCP attribution benches, including
# guest scheduling, payload delivery, no-timer wake state, first-pump outcome,
# and BENCH-124 scheduled-read-pump outcome fields.
DEFAULT_NET_TCP_TRACE_ROWS="connect_close_1,connect_close_4,connect_close_8,echo_1x1,echo_1x5,echo_1x5_string,echo_1x4k,echo_1x64k,echo_1x256k,echo_1x1m,burst_4x1_echo_once,burst_16x1_echo_once,burst_16x1_string_echo_once,burst_64x1_echo_once,burst_16x4096_echo_once,burst_64x1024_echo_once,burst_256x256_echo_once,pingpong_1x1,pingpong_4x1,pingpong_8x1,pingpong_16x1,pingpong_32x1,concurrent_2x1,concurrent_4x1,concurrent_8x1,echo_4x1,echo_8x1"
run "net-tcp-cadence-trace" \
  scripts/benchmarks/net-tcp-event-floor.bench.ts \
    --iterations="${BENCH_NET_TCP_TRACE_ITERATIONS:-5}" \
    --warmup="${BENCH_NET_TCP_TRACE_WARMUP:-1}" \
    --rows="${BENCH_NET_TCP_TRACE_ROWS:-$DEFAULT_NET_TCP_TRACE_ROWS}" \
    --net-bridge-trace \
    ${BENCH_NET_TCP_TRACE_POLL_DELAY_MS:+--net-poll-delay-ms="${BENCH_NET_TCP_TRACE_POLL_DELAY_MS}"}

# Direct WASM command startup/capture floor. Separates command module size,
# warmup-marker cache state, and stdout capture size from shell/directory work.
run "wasm-command-floor" \
  scripts/benchmarks/wasm-command-floor.bench.ts \
    --iterations="${BENCH_WASM_COMMAND_FLOOR_ITERATIONS:-3}" \
    --warmup="${BENCH_WASM_COMMAND_FLOOR_WARMUP:-1}" \
    --serial-runs="${BENCH_WASM_COMMAND_FLOOR_SERIAL_RUNS:-3}" \
    --stdout-sizes="${BENCH_WASM_COMMAND_FLOOR_STDOUT_SIZES:-0,1,65536}" \
    ${BENCH_WASM_COMMAND_FLOOR_WARMUP_DEBUG:+--wasm-warmup-debug}

# WASM runner diagnostics lane. This captures the warmup/phase stderr metrics
# used by the VM-ls investigation without requiring ad-hoc command lines.
run "wasm-command-floor-debug" \
  scripts/benchmarks/wasm-command-floor.bench.ts \
    --iterations="${BENCH_WASM_COMMAND_DEBUG_ITERATIONS:-2}" \
    --warmup="${BENCH_WASM_COMMAND_DEBUG_WARMUP:-0}" \
    --serial-runs="${BENCH_WASM_COMMAND_DEBUG_SERIAL_RUNS:-2}" \
    --stdout-sizes="${BENCH_WASM_COMMAND_DEBUG_STDOUT_SIZES:-0,1}" \
    --wasm-warmup-debug

# Mount-table readdir scaling. Varies unrelated native mounts and direct child
# mounts to isolate mount resolution and child_mount_basenames overhead.
run "mount-readdir" \
  scripts/benchmarks/mount-readdir.bench.ts \
    --iterations="${BENCH_MOUNT_READDIR_ITERATIONS:-20}" \
    --warmup="${BENCH_MOUNT_READDIR_WARMUP:-3}" \
    --mount-counts="${BENCH_MOUNT_READDIR_COUNTS:-0,10,100}" \
    --entry-count="${BENCH_MOUNT_READDIR_ENTRY_COUNT:-32}"

# Overlay readdir scaling. Varies lower/upper/whiteout/opaque overlay states
# through the JS layer-store API to isolate overlay merge cost.
run "overlay-readdir" \
  scripts/benchmarks/overlay-readdir.bench.ts \
    --iterations="${BENCH_OVERLAY_READDIR_ITERATIONS:-20}" \
    --warmup="${BENCH_OVERLAY_READDIR_WARMUP:-5}" \
    --ops-per-sample="${BENCH_OVERLAY_READDIR_OPS_PER_SAMPLE:-100}" \
    --entry-counts="${BENCH_OVERLAY_READDIR_ENTRY_COUNTS:-0,1,32,100,1000}" \
    --modes="${BENCH_OVERLAY_READDIR_MODES:-plain,withFileTypes}"

# Differential process-spawn floor: native baseline -> host node -> guest VM.
if should_run "process-spawn"; then
  build_native_baseline
  export NODE_OPTIONS="${NODE_OPTIONS:---expose-gc}"
fi
run "process-spawn" \
  scripts/benchmarks/process-spawn.bench.ts

# Public process lifecycle attribution. Keeps BENCH-034's native/node/guest
# phases and adds BENCH-035 TS-side trace rows for async start, metadata refresh,
# wait resolution route, and trailing output drain. It also carries the
# BENCH-098 nested child-process parent-vs-guest attribution rows.
if should_run "process-spawn-lifecycle"; then
  build_native_baseline
  export NODE_OPTIONS="${NODE_OPTIONS:---expose-gc}"
  echo "" >&2
  echo "=== Running process-spawn-lifecycle ===" >&2
  BENCH_PROCESS_LIFECYCLE_TRACE=1 \
  BENCH_ITERATIONS="${BENCH_PROCESS_LIFECYCLE_ITERATIONS:-${BENCH_ITERATIONS:-20}}" \
  BENCH_WARMUP="${BENCH_PROCESS_LIFECYCLE_WARMUP:-${BENCH_WARMUP:-5}}" \
  pnpm exec tsx scripts/benchmarks/process-spawn.bench.ts \
    1> "$RESULTS_DIR/process-spawn-lifecycle.json" \
    2> >(tee "$RESULTS_DIR/process-spawn-lifecycle.log" >&2)
else
  echo "=== Skipping process-spawn-lifecycle (BENCH_ONLY=$BENCH_ONLY) ===" >&2
fi

# Session-creation VM-tax benchmark (deterministic, llmock-backed).
# Compares the agentOS VM path vs the bare-node pi-SDK equivalent and gates the
# deterministic metrics against scripts/benchmarks/baseline.json.
# Set BENCH_GATE=1 to fail the run on a regression (CI); set BENCH_UPDATE_BASELINE=1
# to refresh the committed baseline (do this on a clean checkout, review in PR).
echo "" >&2
echo "=== Running session ===" >&2
if should_run "session"; then
  pnpm exec tsx scripts/benchmarks/session.bench.ts --iterations=5 \
    ${BENCH_GATE:+--gate} ${BENCH_UPDATE_BASELINE:+--update-baseline} \
    1> "$RESULTS_DIR/session.json" \
    2> >(tee "$RESULTS_DIR/session.log" >&2)
else
  echo "=== Skipping session (BENCH_ONLY=$BENCH_ONLY) ===" >&2
fi

# Fuzz/perf matrix: native -> host Node -> guest VM latency breadth, fuzz,
# leak, footprint, findings, and regression diff.
if should_run "fuzz-perf"; then
  build_native_baseline
  build_native_baseline_wasm
  export NODE_OPTIONS="${NODE_OPTIONS:---expose-gc}"
  export BENCH_ITERATIONS="${BENCH_FUZZ_PERF_ITERATIONS:-${BENCH_ITERATIONS:-20}}"
  export BENCH_WARMUP="${BENCH_FUZZ_PERF_WARMUP:-${BENCH_WARMUP:-5}}"
  export BENCH_LEAK_CYCLES="${BENCH_FUZZ_PERF_LEAK_CYCLES:-${BENCH_LEAK_CYCLES:-4}}"
  run "fuzz-perf" scripts/benchmarks/run-all.ts
else
  echo "" >&2
  echo "=== Skipping fuzz-perf (BENCH_ONLY=$BENCH_ONLY) ===" >&2
fi

echo "" >&2
echo "=== Done. Results in $RESULTS_DIR ===" >&2
echo "Update website/src/data/bench.ts inputs from these JSON files." >&2
