# Standalone WebAssembly executors

The shipped sidecar runs every WebAssembly command on V8, the same engine that
runs guest JavaScript. Each WASM guest gets its own V8 isolate, and its host
imports are native Rust callbacks; no JavaScript runs in the WASM guest path.

Wasmtime is a second, opt-in engine for Rust embedders of `agentos-vm`. It is
not compiled into the published sidecar. Build it in with the `wasm-wasmtime`
Cargo feature on `agentos-sidecar` or `agentos-vm`; the threaded profile also
needs the executor's `threads` feature. The engines do not share guest memory
and there is no V8-to-Wasmtime bridge.

## Selecting an engine

Omit `wasmBackend` to use the sidecar default, V8. Clients must not invent
another default. A build that includes Wasmtime can select it per execution:

```ts
await vm.execCommand("ls", ["-la"], { wasmBackend: "wasmtime" });
await vm.execCommand("threaded-tool", [], {
  wasmBackend: "wasmtime-threads",
});
await vm.execCommand("ls", ["-la"], { wasmBackend: "v8" });
```

The selector is sealed to `"wasmtime" | "wasmtime-threads" | "v8"`. Selecting
an engine the sidecar was built without fails with
`ERR_AGENTOS_EXECUTOR_UNAVAILABLE`. `"wasmtime"` is the exact single-threaded
profile and rejects shared memory, atomics, and the agentOS thread-spawn
import; so does `"v8"`. Child processes inherit their parent's engine.

## Shared host behavior and safety

Both executors use the same sidecar-owned kernel, VFS, process table, fd and
socket tables, signal broker, permissions, and resource ledger. They also share
one implementation of the agentOS-owned Preview1/POSIX ABI: the host imports in
`agentos-executor-wasm-host` are Rust handlers that each engine binds to its
own guest. V8 instantiates the module from Rust in a per-guest isolate and
calls the handlers from native import callbacks, so no JavaScript runs in the
guest path. Neither engine constructs a `wasmtime-wasi` context or receives
ambient host filesystem, network, process, environment, clock, random, or
stdio authority.

All filesystem and network waits use owned request/result buffers. The shared
handlers validate and copy guest input before awaiting, hold no guest-memory
borrow across the wait, then reacquire and revalidate every output range before
commit. Guest execution runs on the bounded non-Tokio VM executor; host I/O
continues on the process's one Tokio runtime.

The threaded profile puts each guest thread group in a dedicated killable
worker process. Wasmtime Engines, Stores, Instances, shared linear memory, and
native guest threads live in that child; the kernel, VFS, descriptors, sockets,
permissions, process table, and signal dispositions remain authoritative in
the parent. A bounded typed protocol carries owned host-operation values over
stdio. The child receives no ambient host capability, and guest memory is
never mapped into the parent as an IPC shortcut.

Both engines run the same cross-engine parity and safety suites. agentOS never
shadow-runs a side-effecting command through both engines.

## Metrics and warnings

`getResourceSnapshot()` exposes these process/VM diagnostics:

| Field | Meaning |
| --- | --- |
| `wasmReservedMemoryBytes` | Live ledger charge for WASM linear memory, tables, and Wasmtime async stacks. It must return to zero after execution teardown. |
| `wasmtimeEngineProfiles` | Process-wide exact Engine profiles retained for distinct stack/feature configurations. |
| `wasmtimeModuleEntries` | Compiled modules currently retained across Engine-profile caches. |
| `wasmtimeModuleCacheHits`, `wasmtimeModuleCacheMisses`, `wasmtimeModuleCacheEvictions` | Cumulative cache behavior. A rising miss or eviction rate predicts cold-start latency. |
| `wasmtimeCompiledSourceBytes` | Cumulative source bytes compiled; this is a counter, not current resident memory. |
| `wasmtimeChargedModuleBytes` | Conservative current compiled-module cache charge used for bounded admission/eviction. |
| `wasmtimeCompileTimeMicros` | Cumulative synchronous Wasmtime compilation time. |
| `wasmtimeProcessRetainedRssBytes` | Whole-sidecar process RSS sampled on Linux. It is intentionally not presented as Wasmtime-only RSS. |

Operators should alert on sustained module-cache misses/evictions, nonzero
`wasmReservedMemoryBytes` after all executions drain, or profile counts near
the configured bound. The runtime emits host-visible warnings before the
bounded Engine-profile and module-cache limits and typed errors at the limit:

- `WARN_AGENTOS_WASMTIME_ENGINE_PROFILES_NEAR_LIMIT` / `ERR_AGENTOS_WASMTIME_ENGINE_PROFILE_LIMIT`
- `WARN_AGENTOS_WASMTIME_MODULE_CACHE_NEAR_LIMIT` / `ERR_AGENTOS_WASMTIME_MODULE_CACHE_LIMIT`
- `WARN_AGENTOS_WASMTIME_LIMIT_WARNING` for aggregate Store reservations
- `WARN_AGENTOS_RESOURCE_NEAR_LIMIT` / `ERR_AGENTOS_WASM_THREAD_LIMIT` for
  per-VM and process-wide threaded-WASM admission

Limit errors include `limitName`, `limit`, and `observed` details where those
values apply. Guest traps use `ERR_AGENTOS_WASM_TRAP` plus a stable `trapKind`
(`unreachable`, `memory-out-of-bounds`, `table-out-of-bounds`,
`array-out-of-bounds`, `misaligned-atomic`, `null-indirect-call`,
`indirect-call-type-mismatch`, `integer-overflow`, `integer-division-by-zero`,
`invalid-float-to-integer`, `null-reference`, `cast-failure`,
`allocation-too-large`, or `other`). Memory, table, stack, active-CPU, fuel,
wall-clock, cancellation, invalid-module, and instantiation outcomes have their
own `ERR_AGENTOS_WASM_*` codes. Both engines report a shared outcome with the
same code. A code with an engine prefix (`ERR_AGENTOS_WASMTIME_*` or
`ERR_AGENTOS_V8_*`) names a failure in that engine's own machinery, such as
Wasmtime's thread worker processes or V8's isolate heap. Raw engine validation
or trap strings are private diagnostics and are not an API contract.

## Compilation, cold start, and memory

The Wasmtime backend performs ordinary in-process compilation and keeps a
bounded, SHA-256-keyed `Module` LRU per exact Engine profile. Cache input is the
original trusted module bytes and the configured feature/stack profile; there
is no serialized or externally supplied native artifact.

The production configuration uses on-demand linear-memory allocation and
Wasmtime's eligible copy-on-write module-memory initialization. It does **not**
use pooling allocation, AOT deserialization, Wizer, or a live Store/Instance
snapshot. Wasmtime does not provide a general live-process snapshot/fork for
agentOS: copying linear memory alone would omit kernel process, fd, socket,
signal, waiter, and thread state. V8's JavaScript heap snapshot remains a
JavaScript startup optimization and is not a cross-engine WASM snapshot.

Memory figures must be kept separate:

- RSS/PSS measure committed process memory; the benchmark records baseline,
  peak, end, and retained-after-teardown values from `/proc`.
- VIRT includes Wasmtime guard/address-space reservations and is not committed
  memory. It can rise sharply at concurrency without an equivalent RSS rise.
- `guestLinearMemoryBytes`, `asyncStackBytes`, and `reservedStoreBytes` are
  recorded per Wasmtime execution in opt-in phase diagnostics.
- compiled-module charge and kernel buffered bytes are reported separately;
  neither is guest linear memory.
- an active `wasmtime-threads` group also has a dedicated child-process image,
  two fixed IPC support threads, and one Store/Instance/native stack for each
  admitted guest thread. This overhead is intentionally not conflated with the
  ordinary single-thread Wasmtime RSS numbers below.

## Benchmarks

Current V8 and Wasmtime measurements, including first and warm runs,
concurrency, memory, and the per-syscall latency matrix, are in
[`docs/design/v8-native-wasm-imports.md`](../design/v8-native-wasm-imports.md)
with raw medians in
[`packages/benchmarks/results/v8-native-wasm-imports-20261122.json`](../../packages/benchmarks/results/v8-native-wasm-imports-20261122.json).
Run the engine comparison from the repository root with a release sidecar built
with `--features wasm-wasmtime` and rebuilt canonical commands:

```bash
AGENTOS_SIDECAR_BIN=/absolute/path/to/release/agentos-sidecar \
AGENTOS_WASM_COMMANDS_DIR=/absolute/path/to/packages/core/commands \
pnpm --dir packages/benchmarks bench:wasm-backends
```

## Threads

Shared WebAssembly memory and pthreads are enabled only by the explicit
`"wasmtime-threads"` profile. agentOS does not rely on shared memory between V8
isolates, and no memory is shared between V8 and Wasmtime. Threaded programs use
the owned agentOS sysroot/libc and import `wasi.thread-spawn`; each pthread gets
its own Wasmtime Store and Instance over one group-owned `SharedMemory`.

Before any guest code runs, admission transactionally reserves the configured
`limits.wasm.maxThreads` (including the initial thread), maximum shared-memory
envelope, table capacity, async stacks, and native thread capacity. The
per-group default is 16 threads, `limits.wasm.maxConcurrentThreads` bounds the
aggregate reservations of concurrent groups in one VM at 64 by default, and
the process-wide default is 256. Capacity failure is typed and starts no
partial group. Shared-memory growth stays within the pre-reserved maximum, and
atomics, wait/notify, and cross-Store mutation operate on the group-owned
memory.

Signal dispositions and process-pending signals remain process-wide in the
kernel. Masks, temporary `ppoll` masks, and in-progress handler state are
per-thread; process-directed delivery deterministically selects an unblocked
thread. Trap, process exit, cancellation, timeout, `SIGKILL`, and VM disposal
terminate and reap the entire child process, including threads indefinitely
parked in `memory.atomic.wait`, without terminating the sidecar.

The threaded libc covers mutexes, condition variables, TLS, join/detach,
thread exit, and cooperative cancellation. Live Store/Instance snapshots,
cross-engine memory, AOT artifacts, Wizer, pooling, components, and browser
execution remain unsupported. Main-thread exit tears down the group; detached
guest threads do not outlive the command.
