# agentOS JS-layer benchmarks

Benchmarks that run from the TypeScript layer (the surface real consumers use:
`AgentOs.create()` / `createSession()`), under a local **llmock** so the
infrastructure metrics are deterministic and gate-able.

## Benchmarks

| file | what it measures |
|---|---|
| `coldstart.bench.ts` | total cold-start for `echo` / `pi-session` / `pi-prompt-turn` / `claude-session` |
| `echo.bench.ts` | WASM shell `echo` cold-start and warm-start command floor across sequential/concurrent batches |
| `memory.bench.ts` | per-session memory (RSS/heap) |
| `session.bench.ts` | **session-creation "VM tax"**: agentOS VM path vs the bare-node pi-SDK equivalent |
| `ls.bench.ts` | VM create + setup + serial `ls`, compared with the same host `ls` workload |
| `wasi-ls-scaling.bench.ts` | focused coreutils/WASI `ls` scaling with fixtures prepared outside the timed loop |
| `process-spawn.bench.ts` | differential process-spawn floor: native baseline, host Node child process, and guest VM spawn |
| `readdir.bench.ts` | pure `fs.readdirSync` scaling for plain and `withFileTypes` directory reads over VM-shadow and native `host_dir` fixtures |
| `fs-sync-ops.bench.ts` | focused synchronous filesystem operation floor with logical call counts and expected sync-RPC counts |
| `sync-bridge-floor.bench.ts` | pure benchmark-only sync bridge/noop floor before filesystem or VFS dispatch |
| `dns-lookup-floor.bench.ts` | focused DNS lookup floor across warm, repeated, concurrent, and fresh-process `localhost` lookups |
| `net-tcp-event-floor.bench.ts` | focused TCP loopback/event floor across connect, poll cadence, write count, payload size, and concurrency |
| `wasm-command-floor.bench.ts` | direct WASM command startup/capture floor across command sizes and stdout sizes |
| `mount-readdir.bench.ts` | native mount-table readdir scaling for unrelated mounts and child mounts |
| `overlay-readdir.bench.ts` | JS overlay filesystem readdir scaling across lower/upper/whiteout/opaque states |
| `run-all.ts` | broad fuzz/perf sweep: process/net/fs/dns/pipes/control latency matrix, fuzz findings, leak detection, footprint, and baseline diff |

## Standard suite format

`scripts/benchmarks/run-benchmarks.sh` is the repo-standard suite entrypoint.
It is not a third-party benchmark schema: each lane is a TypeScript benchmark
runner, and the shell wrapper writes the normalized artifacts to:

- `scripts/benchmarks/results/<lane>.json`
- `scripts/benchmarks/results/<lane>.log`

Run one lane with `BENCH_ONLY=<lane> bash scripts/benchmarks/run-benchmarks.sh`.
Run without `BENCH_ONLY` to execute the full standard suite. The standard lanes
are:

- `coldstart-sleep`
- `echo-cold-warm`
- `memory-sleep`
- `memory-pi-session`
- `ls-serial`
- `wasi-ls-scaling`
- `wasi-ls-scaling-counters`
- `readdir-scaling`
- `readdir-probe`
- `fs-sync-ops`
- `fs-sync-ops-phases`
- `sync-bridge-floor`
- `sync-bridge-floor-phases`
- `dns-lookup-floor`
- `net-tcp-event-floor`
- `net-tcp-cadence-trace`
- `wasm-command-floor`
- `wasm-command-floor-debug`
- `mount-readdir`
- `overlay-readdir`
- `process-spawn`
- `process-spawn-lifecycle`
- `session`
- `fuzz-perf`

### `ls.bench.ts` — VM startup + serial `ls`

This is the benchmark from Codex thread `019f05eb-fc28-7e90-9620-7b5a2150cb9a`
promoted into the standard suite. It creates a VM with coreutils, prepares an
empty and 100-file directory, runs `ls` repeatedly in serial, and records the
same host `ls` baseline.

```bash
tsx scripts/benchmarks/ls.bench.ts
tsx scripts/benchmarks/ls.bench.ts --iterations=10 --serial-runs=25 --file-counts=0,100
tsx scripts/benchmarks/ls.bench.ts --wasm-warmup-debug
```

The result also includes `vmNoopExec`, `vmStdoutExec`, `vmLsMinusNoopMs`,
`vmLsMinusStdoutMs`, and `lsDeltaFromEmptyMs` so process/shell/stdout floor can
be separated from directory-size cost. `--wasm-warmup-debug` is opt-in and
passes `AGENTOS_WASM_WARMUP_DEBUG=1` to the measured commands so stderr/logs can
show whether the WASM warmup marker executed or hit cache.

### `wasi-ls-scaling.bench.ts` — focused coreutils `ls` scaling

This benchmark prepares host and guest directory fixtures once, reuses a single
VM, and runs coreutils via direct `execArgv("ls", ["-1", dir])`. It records host
`ls -1`, guest `true`, guest `ls -1`, `ls-true`, empty-directory deltas, stdout
bytes, stdout callback chunks, and observed stdout name count versus fixture
count. Use this when the question is the residual directory-size slope rather
than VM startup/setup cost. The optional `--ls-variants` list can compare
`one`, `unsorted`, `no-color`, `unsorted-no-color`, and `fast-no-decor` to
separate coreutils display/sort/decor policy from runtime stat overhead.
`--wasi-syscall-counters` is opt-in and records stderr metrics from the WASI
runner for `fd_readdir`, `path_open`, `path_filestat_get`, `fd_filestat_get`,
and `fd_write`.

```bash
tsx scripts/benchmarks/wasi-ls-scaling.bench.ts
tsx scripts/benchmarks/wasi-ls-scaling.bench.ts --iterations=10 --file-counts=0,1,32,100,1000
tsx scripts/benchmarks/wasi-ls-scaling.bench.ts --ls-variants=one,unsorted-no-color,fast-no-decor --file-counts=1000
tsx scripts/benchmarks/wasi-ls-scaling.bench.ts --wasm-warmup-debug
tsx scripts/benchmarks/wasi-ls-scaling.bench.ts --wasi-syscall-counters --file-counts=0,100,1000
BENCH_ONLY=wasi-ls-scaling-counters bash scripts/benchmarks/run-benchmarks.sh
```

### `readdir.bench.ts` — pure readdir scaling

This benchmark isolates directory-read cost by preparing fixtures outside the
timed loop, then measuring host Node and guest VM `fs.readdirSync` over the same
entry counts. The default `vm-shadow` fixture creates files inside the VM. The
`native-host-dir` fixture mounts the prepared host directory through the native
`host_dir` plugin so mapped-directory `withFileTypes` cost can be compared
directly. The default `pure` workload measures only `readdirSync`; the
`matrix-guarded` workload repeats the broad fuzz/perf shape by checking the
directory and each expected child before reading it. The opt-in `probe` workload
adds explicit preflight dimensions so sync-call tax can be separated from
enumeration cost. It records `fixture`, `workload`, `preflightOp`,
`preflightCount`, `includeReaddir`, `operationCounts`, `entryCount`, `mode`,
`returnedCount`, `payloadBytes`, `msPerEntry`, `deltaFromEmptyMs`, and derived
gap fields.

```bash
tsx scripts/benchmarks/readdir.bench.ts
tsx scripts/benchmarks/readdir.bench.ts --iterations=20 --entry-counts=0,1,32,100,1000 --modes=plain,withFileTypes
tsx scripts/benchmarks/readdir.bench.ts --fixtures=vm-shadow,native-host-dir --entry-counts=0,100,1000 --modes=withFileTypes
tsx scripts/benchmarks/readdir.bench.ts --workloads=pure,matrix-guarded --entry-counts=0,32 --modes=plain
tsx scripts/benchmarks/readdir.bench.ts --workloads=probe --entry-counts=32 --modes=plain --preflight-ops=none,existsSync,statSync --preflight-counts=0,32,33 --include-readdir=both --probe-targets=dir-plus-children
BENCH_ONLY=readdir-probe bash scripts/benchmarks/run-benchmarks.sh
```

### `fs-sync-ops.bench.ts` — sync filesystem operation floor

This benchmark separates logical Node `fs` operations from the expected number
of synchronous bridge RPCs they trigger. It covers the small filesystem rows that
remain prominent in the broad fuzz/perf matrix after `readdir_large` is explained
by repeated sync preflight calls.

```bash
tsx scripts/benchmarks/fs-sync-ops.bench.ts
tsx scripts/benchmarks/fs-sync-ops.bench.ts --ops=existsSync,statSync,openClose,mkdirRmdir,smallWrite,readFileSync,renameFile --call-counts=1,8,32 --payload-bytes=8
tsx scripts/benchmarks/fs-sync-ops.bench.ts --ops=statSync --call-counts=32 --sync-rpc-latency
tsx scripts/benchmarks/fs-sync-ops.bench.ts --ops=statSync --call-counts=32 --sync-rpc-latency --fs-sync-phases
BENCH_ONLY=fs-sync-ops-phases bash scripts/benchmarks/run-benchmarks.sh
```

### `sync-bridge-floor.bench.ts` — pure sync bridge/noop floor

This benchmark calls a benchmark-only no-op sync bridge RPC from guest Node. The
sidecar returns before filesystem/VFS dispatch, so the result isolates bridge
round-trip, request serialization, service-loop routing, response encoding, and
V8 deserialization overhead. Use it before optimizing VFS/path lookup for
small sync filesystem rows.

```bash
tsx scripts/benchmarks/sync-bridge-floor.bench.ts
tsx scripts/benchmarks/sync-bridge-floor.bench.ts --call-counts=1,8,32 --payload-bytes=0 --sync-rpc-latency
tsx scripts/benchmarks/sync-bridge-floor.bench.ts --call-counts=8 --sync-rpc-latency --bridge-phases
BENCH_ONLY=sync-bridge-floor BENCH_SYNC_BRIDGE_RPC_LATENCY=1 bash scripts/benchmarks/run-benchmarks.sh
BENCH_ONLY=sync-bridge-floor BENCH_SYNC_BRIDGE_RPC_LATENCY=1 BENCH_SYNC_BRIDGE_PHASES=1 bash scripts/benchmarks/run-benchmarks.sh
BENCH_ONLY=sync-bridge-floor-phases bash scripts/benchmarks/run-benchmarks.sh
```

### `dns-lookup-floor.bench.ts` — DNS lookup floor

This benchmark decomposes the broad `dns/*` matrix rows into warm single
lookup, sequential repeated same-host lookups, concurrent same-host lookups, and
fresh-process lookup shapes. It records total p50, per-lookup cost, and
first-versus-rest timing for sequential rows so resolver setup cost can be
separated from repeated lookup cost.

```bash
tsx scripts/benchmarks/dns-lookup-floor.bench.ts
tsx scripts/benchmarks/dns-lookup-floor.bench.ts --rows=single_localhost,sequential_same_32,concurrent_same_16,cold_process_single
BENCH_ONLY=dns-lookup-floor bash scripts/benchmarks/run-benchmarks.sh
```

### `net-tcp-event-floor.bench.ts` — TCP loopback event floor

This benchmark decomposes the broad TCP rows into connect/accept, poll cadence,
write count, payload size, reply mode, and client concurrency dimensions while
keeping the same in-VM localhost shape. Rows include `costAxis`,
`compareAgainst`, and `completionSemantics` metadata so focused runs can be
matched back to the broad matrix or compared against a simpler baseline row.
Use `--net-bridge-trace` / `BENCH_NET_TCP_TRACE=1` for the counter-backed
`net-tcp-cadence-trace` variant, which records guest bridge raw reads, timeout
sentinels, accept polls, emitted socket events, write coalescing counters, and
payload encode/decode timing. It also records sidecar/kernel readiness and
socket read/write elapsed counters under `sidecarNetTrace`.
Trace runs can also set `--net-poll-delay-ms` / `BENCH_NET_TCP_POLL_DELAY_MS`
to sweep the benchmark-only TCP bridge poll delay while leaving the default
runtime delay unchanged.
For same-binary copy-elision A/B checks, set
`BENCH_NET_RETAIN_OWNED_WRITE_BUFFER=0` to force the guest bridge to copy
owned string-write buffers before queueing them.
The standard `net-tcp-cadence-trace` lane runs the full focused TCP row set:
connect/accept scaling, buffer/string echo rows, payload-size rows, burst-write
rows, ping-pong cadence rows, and concurrent client rows. Use
`BENCH_NET_TCP_TRACE_ROWS` only to narrow a local investigation.
It is also the suite home for the later TCP attribution benches: guest
scheduling, payload delivery, post-delivery probes, no-timer wake state,
first-pump result attribution, and BENCH-124 scheduled-read-pump outcome fields.

```bash
tsx scripts/benchmarks/net-tcp-event-floor.bench.ts
tsx scripts/benchmarks/net-tcp-event-floor.bench.ts --rows=connect_close_1,connect_close_8,echo_1x1,echo_1x256k,burst_64x1024_echo_once,pingpong_32x1,echo_8x1
tsx scripts/benchmarks/net-tcp-event-floor.bench.ts --net-bridge-trace --rows=connect_close_1,connect_close_8,echo_1x5,echo_1x64k,burst_16x1_echo_once,pingpong_16x1,pingpong_32x1,concurrent_4x1,concurrent_8x1,echo_8x1
tsx scripts/benchmarks/net-tcp-event-floor.bench.ts --net-bridge-trace --net-poll-delay-ms=2 --rows=connect_close_1,echo_1x5,concurrent_4x1,pingpong_4x1
BENCH_ONLY=net-tcp-cadence-trace bash scripts/benchmarks/run-benchmarks.sh
```

### `process-spawn.bench.ts` — process-spawn floor

This benchmark compares the same `node -e 'process.exit(0)'` operation across
the native Rust baseline, host Node `child_process`, and guest VM `spawn`. It
reuses one VM for the guest lane so the measured cost is per-spawn isolate/process
work, not per-VM boot. The `process-spawn-lifecycle` lane enables BENCH-035
trace output, splitting the guest public-process `wait_reap` bucket into
TypeScript-side async start, execute RPC, signal metadata refresh, wait
resolution route, snapshot fallback, and trailing output drain. BENCH-069 extends
that same standard lane with exit critical-path timestamps for event receipt,
finish processing, wait promise observation, wait return, and derived
unattributed wait gaps. Recent process optimization attribution is also emitted
from that lane under the `processLifecycle` JSON object:
`sidecarJsEventPhases`, `hostWriteSyncRows`, `fanoutLadderRows`, and
`nestedChildProcessRows`. The nested child-process row is BENCH-098's standard
suite home: it measures one public guest parent process that spawns child
processes inside the VM, and compares that with the same host Node parent
shape.

```bash
tsx scripts/benchmarks/process-spawn.bench.ts
BENCH_ITERATIONS=20 BENCH_WARMUP=5 tsx scripts/benchmarks/process-spawn.bench.ts
BENCH_ONLY=process-spawn-lifecycle bash scripts/benchmarks/run-benchmarks.sh
```

The standard suite output for this lane is
`scripts/benchmarks/results/process-spawn-lifecycle.json`, with the matching log
at `scripts/benchmarks/results/process-spawn-lifecycle.log`.

### `wasm-command-floor.bench.ts` — command startup/capture floor

This benchmark measures direct `execArgv` without shell parsing. It records
first-run p50, warm-repeat p50, command module byte size, stdout byte count,
stdout callback chunk counts, and optional WASM warmup diagnostics for a
command-size ladder and a same-module `printf` stdout-size sweep.

```bash
tsx scripts/benchmarks/wasm-command-floor.bench.ts
tsx scripts/benchmarks/wasm-command-floor.bench.ts --stdout-sizes=0,1,1024,4096,16384,65536,262144
tsx scripts/benchmarks/wasm-command-floor.bench.ts --serial-runs=5 --wasm-warmup-debug
BENCH_ONLY=wasm-command-floor-debug bash scripts/benchmarks/run-benchmarks.sh
```

### `mount-readdir.bench.ts` — mount-table readdir scaling

This benchmark varies native `host_dir` mount count and times `AgentOs.readdir`
for two cases: a fixed mounted target with many unrelated mounts, and a parent
directory with many direct child mounts. It is the focused benchmark for
mount-table normalization / child-mount index ideas.

```bash
tsx scripts/benchmarks/mount-readdir.bench.ts
tsx scripts/benchmarks/mount-readdir.bench.ts --mount-counts=0,10,100,1000 --entry-count=32
```

### `overlay-readdir.bench.ts` — overlay readdir scaling

This benchmark times the exported JS overlay/layer API directly, without VM
creation or WASM command startup in the timed loop. It covers raw in-memory
filesystem control, clean one-lower and two-lower overlays, empty upper,
upper/lower merge, upper shadows, whiteouts, and opaque directories. Timings are
reported per operation, batched by `--ops-per-sample` to keep sub-millisecond
reads out of timer noise.

```bash
tsx scripts/benchmarks/overlay-readdir.bench.ts
tsx scripts/benchmarks/overlay-readdir.bench.ts --entry-counts=0,1,32,100,1000 --ops-per-sample=100
```

### `run-all.ts` — fuzz/perf sweep

The fuzz/perf sweep measures the same logical operation across three layers
(`native` -> `host node` -> `guest VM`) and writes:

- `results/latency-matrix.json`
- `results/fuzz-findings.json`
- `results/leak-process.json`
- `results/footprint.json`
- `results/findings.json`
- `results/regression-diff.json`

It also emits the full `findings.json` payload to stdout so `run-benchmarks.sh`
captures `results/fuzz-perf.json` like the rest of the suite.

### `session.bench.ts` — the VM-tax benchmark

Two lanes, same llmock, same timer:

- **`vm`** — `AgentOs.create()` (`vmCreate`) + `createSession("pi")` (`sessionCreate`).
- **`bare-node`** — the *same* pi-SDK session construction on host node, **no VM**
  (`sessionCreate` = load pi SDK + `createAgentSession`). This is the "Node.js
  equivalent" baseline; it mirrors `../secure-exec/registry/agent/pi/src/adapter.ts` `newSession`.

Derived metrics:

- `derived.vmTaxMs` = `vm.sessionCreate.p50 − bareNode.sessionCreate.p50`
- `derived.vmTaxRatio` = `vm.sessionCreate.p50 / bareNode.sessionCreate.p50` (hardware-independent)

Prompt latency is **not** measured here — it's LLM-bound and belongs in a separate
informational real-API suite, never in the deterministic gate.

```bash
tsx scripts/benchmarks/session.bench.ts                   # run, print delta vs baseline
tsx scripts/benchmarks/session.bench.ts --lanes=vm        # one lane only
tsx scripts/benchmarks/session.bench.ts --gate            # exit non-zero on regression (CI)
tsx scripts/benchmarks/session.bench.ts --update-baseline # refresh baseline.json (review the diff!)
```

The bare-node lane is skipped with a clear message if `@mariozechner/pi-coding-agent`
isn't resolvable on the host (it's a devDependency for exactly this reason).

## Baselines & regression gating

Strategy: a committed **`baseline.json`** (golden numbers + full metadata) +
**relative-threshold** comparison.

- The gate runs only on **deterministic, llmock-backed** metrics (`vmCreate`,
  `sessionCreate`, `vmTaxRatio`) — never on LLM-bound latency.
- Thresholds are **relative** (e.g. +12% on `sessionCreate.p50`) so the gate
  tolerates within-class hardware drift, plus a **noise floor** (absolute ms) so
  tiny/fast metrics don't flake on sub-ms jitter.
- `vmTaxRatio` is gated as a **hardware-independent** signal — it survives
  cross-machine variance far better than absolute latencies.

Gate rules live in `session.bench.ts` (`GATE_RULES`); the comparison/IO logic
is in `baseline.ts` (reusable across benches).

### Updating the baseline

Regenerate on a **clean checkout in the canonical environment** (CI runner or a
clean `main` install — *not* a dev workspace in `secure-exec-local` mode, whose
numbers and dep versions are unrepresentative):

```bash
pnpm install            # clean, pinned deps
pnpm build
tsx scripts/benchmarks/session.bench.ts --update-baseline
# review baseline.json — check gitSha, deps versions, gitDirty:false, hardware
```

Each baseline records `gitSha`, `deps` versions (these move the numbers — a
published binary vs a source build differ a lot), hardware, node version, llmock
flag, and full percentiles. Treat a baseline as only comparable on matching
hardware + dep versions.

## CI wiring

`run-benchmarks.sh` runs the suite and writes `results/*.json`. Toggle gating with
env vars:

```bash
BENCH_GATE=1 bash scripts/benchmarks/run-benchmarks.sh             # fail on regression
BENCH_UPDATE_BASELINE=1 bash scripts/benchmarks/run-benchmarks.sh  # refresh baseline
BENCH_ONLY=echo-cold-warm bash scripts/benchmarks/run-benchmarks.sh # run just the WASM echo cold/warm benchmark
BENCH_ONLY=ls-serial bash scripts/benchmarks/run-benchmarks.sh     # run just the VM ls benchmark
BENCH_ONLY=ls-serial BENCH_LS_WASM_WARMUP_DEBUG=1 bash scripts/benchmarks/run-benchmarks.sh # include WASM warmup diagnostics
BENCH_ONLY=wasi-ls-scaling bash scripts/benchmarks/run-benchmarks.sh # run focused coreutils/WASI ls scaling
BENCH_ONLY=wasi-ls-scaling BENCH_WASI_LS_VARIANTS=one,unsorted-no-color,fast-no-decor bash scripts/benchmarks/run-benchmarks.sh # compare ls display/sort variants
BENCH_ONLY=wasi-ls-scaling BENCH_WASI_LS_SYSCALL_COUNTERS=1 bash scripts/benchmarks/run-benchmarks.sh # include WASI syscall counters
BENCH_ONLY=readdir-scaling bash scripts/benchmarks/run-benchmarks.sh # run just pure readdir scaling
BENCH_ONLY=readdir-scaling BENCH_READDIR_FIXTURES=vm-shadow,native-host-dir bash scripts/benchmarks/run-benchmarks.sh # include native host_dir fixture
BENCH_ONLY=readdir-scaling BENCH_READDIR_WORKLOADS=pure,matrix-guarded BENCH_READDIR_ENTRY_COUNTS=0,32 BENCH_READDIR_MODES=plain bash scripts/benchmarks/run-benchmarks.sh # compare broad matrix guard shape
BENCH_ONLY=readdir-scaling BENCH_READDIR_WORKLOADS=probe BENCH_READDIR_ENTRY_COUNTS=32 BENCH_READDIR_MODES=plain BENCH_READDIR_PREFLIGHT_OPS=none,existsSync BENCH_READDIR_PREFLIGHT_COUNTS=0,32,33 BENCH_READDIR_INCLUDE_READDIR=both BENCH_READDIR_PROBE_TARGETS=dir-plus-children bash scripts/benchmarks/run-benchmarks.sh # separate sync-call count from readdir
BENCH_ONLY=fs-sync-ops BENCH_FS_SYNC_CALL_COUNTS=1,8,32 bash scripts/benchmarks/run-benchmarks.sh # separate small sync fs op cost from expected bridge call count
BENCH_ONLY=fs-sync-ops BENCH_FS_SYNC_RPC_LATENCY=1 BENCH_FS_SYNC_PHASES=1 BENCH_FS_SYNC_OPS=statSync bash scripts/benchmarks/run-benchmarks.sh # include sync bridge and sidecar fs dispatch timing
BENCH_ONLY=dns-lookup-floor bash scripts/benchmarks/run-benchmarks.sh # separate DNS warm/repeated/concurrent/fresh-process lookup cost
BENCH_ONLY=dns-lookup-floor BENCH_DNS_LOOKUP_ROWS=single_localhost,sequential_same_32,concurrent_same_16 bash scripts/benchmarks/run-benchmarks.sh # run selected DNS lookup floor rows
BENCH_ONLY=net-tcp-event-floor BENCH_NET_TCP_ROWS=connect_close_1,connect_close_8,echo_1x1,echo_1x256k,burst_64x1024_echo_once,pingpong_32x1,echo_8x1 bash scripts/benchmarks/run-benchmarks.sh # separate TCP connect/payload/write/cadence/client dimensions
BENCH_ONLY=net-tcp-event-floor BENCH_NET_TCP_TRACE=1 BENCH_NET_TCP_ROWS=connect_close_1,echo_1x5,echo_1x64k,concurrent_4x1,pingpong_4x1 bash scripts/benchmarks/run-benchmarks.sh # include guest bridge cadence counters
BENCH_ONLY=net-tcp-event-floor BENCH_NET_TCP_TRACE=1 BENCH_NET_TCP_POLL_DELAY_MS=2 BENCH_NET_TCP_ROWS=connect_close_1,echo_1x5,concurrent_4x1,pingpong_4x1 bash scripts/benchmarks/run-benchmarks.sh # sweep benchmark-only net poll delay
BENCH_ONLY=net-tcp-cadence-trace bash scripts/benchmarks/run-benchmarks.sh # run the full standard counter-backed TCP trace row set
BENCH_ONLY=process-spawn BENCH_ITERATIONS=20 BENCH_WARMUP=5 bash scripts/benchmarks/run-benchmarks.sh # run process-spawn floor
BENCH_ONLY=wasm-command-floor bash scripts/benchmarks/run-benchmarks.sh # run direct WASM command floor
BENCH_ONLY=wasm-command-floor BENCH_WASM_COMMAND_FLOOR_STDOUT_SIZES=0,1,1024,4096,16384,65536,262144 bash scripts/benchmarks/run-benchmarks.sh # stdout ladder
BENCH_ONLY=mount-readdir bash scripts/benchmarks/run-benchmarks.sh # run native mount-table readdir scaling
BENCH_ONLY=overlay-readdir bash scripts/benchmarks/run-benchmarks.sh # run JS overlay readdir scaling
BENCH_ONLY=fuzz-perf bash scripts/benchmarks/run-benchmarks.sh     # run just the fuzz/perf sweep
```

For trend history / PR-comment deltas, layer `github-action-benchmark` or CodSpeed
on top later — both consume the result JSON this harness already emits.

## Notes

- `baseline.json` in this repo may be a dev-workspace seed — regenerate it per
  above before trusting the gate in CI.
- The phase sub-breakdown (`loadPiSdkRuntime`, `resourceLoader.reload`,
  `createAgentSession`) that explains *why* `sessionCreate` is ~1.5s is available
  from the sidecar's `AGENTOS_LOG_FILE` (`kind=create_session elapsed_ms=…`) and
  the `perf`-target phase tracing; wire it into the result JSON if you want the
  breakdown gated too.
