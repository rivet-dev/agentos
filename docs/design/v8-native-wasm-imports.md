# Native V8 WASM imports

Status: implemented; the JavaScript WASM runner is removed

Date: 2026-11-22

Audience: agentOS runtime, kernel, and security maintainers

## Decision

Keep V8 as the default standalone WASM engine, and bind agentOS's host imports
to V8 natively in Rust instead of through the JavaScript runner in
`crates/executor-v8-runtime/assets/runners/wasm-runner.mjs`.

The host imports are not rewritten. The Wasmtime executor already implements
every import in the ABI manifest in Rust, against the same host methods the
runner calls. Those handlers move into an engine-neutral crate that both
engines use, so V8 and Wasmtime behave identically by construction.

V8 is bound through agentOS's existing V8 binding (`crates/v8`) and V8's public
API: a native Rust function per import, a compiled module shared across
isolates, and guest memory read through the exported memory's backing store.
No JavaScript source runs in the guest path. This needs no change to the V8
archives or their build.

Wasmtime remains an opt-in executor. It is not recommended for production; see
[the Wasmtime adoption decision](./wasmtime-adoption-decision.md).

## Why V8

1. **agentOS already ships V8.** JavaScript requires it, so V8 is in every
   sidecar regardless of the WASM engine. One engine means one compiler and
   runtime to build, patch, and review.
2. **Wasmtime would not remove exposure.** agentOS keeps `WebAssembly` enabled
   for guest JavaScript, because packages depend on it, and a guest that can
   run a command can also run `node`. V8's WASM engine is reachable either
   way. Running commands on Wasmtime adds Wasmtime's bugs to what an attacker
   can choose from and removes none of V8's.
3. **V8's WASM engine is far more widely attacked and tested.** It runs
   untrusted WebAssembly for every Chrome user and is a standing target for
   browser exploit teams, Chrome's bug bounty, and contests such as Pwn2Own.
   Wasmtime received comparable outside attention only in 2026. The scrutiny
   cuts both ways: V8 has had at least eight severe WASM bugs that a guest
   module alone could trigger since 2022, and Wasmtime has published six
   critical sandbox escapes since 2021. Staying on a supported V8 is the main
   security control. A V8 branch stops receiving fixes when the next one
   ships.
4. **V8 is faster on sustained compute, at the cost of slower first runs.**
   See the measurements below.

## Measurements

The comparison ran 14 real agentOS command binaries to completion through one
Rust host implementation bound to each engine, plus heavy workloads over
50,000 files, a packed git repository, and DuckDB. Every engine produced
identical output. Wasmtime started from precompiled code and V8 from a warm
isolate where noted. The harness and raw results are in the standalone
`v8-wasm` research repository, not in agentOS.

| What | V8 | Wasmtime |
| --- | --- | --- |
| Sustained compute (DuckDB, `sqlite3`, `jq`), warm | 1.2 to 2.3 times faster | |
| File-heavy and syscall-heavy work (`grep -r`, `rg`, `find` over 50,000 files; `git`), warm | Within 5% | Within 5% |
| One host call | 5 to 9 ns | 6 to 17 ns |
| First run with precompiled Wasmtime code | | 1.3 to 3.5 times faster |
| First run with nothing prepared | 1.2 to 19 times faster | Compiles the whole binary first |
| CPU on a first run | 2 to 8 times more, from background compilation | Less |
| Memory per process | 20 to 50 MiB more | Less |
| Memory per additional live guest | About 2 MiB | About 0.3 MiB |
| Setting up one guest | 0.3 to 4.8 ms with a reused isolate, about 1 ms more with a new one | 0.05 to 0.1 ms |

The JavaScript runner is the largest cost today. A warm `true` takes about
72 ms on the V8 path and about 17 ms on the Wasmtime path, of which the
Wasmtime executor itself is 1.2 ms. Replacing the runner moves V8 commands
toward that floor; the per-guest differences above are small next to it.

V8's public callback interface boxes each argument as a JavaScript value. A
direct native hook measured 12 to 19% faster only on commands making tens of
thousands of host calls, and the same within 6% on whole commands. The public
interface is used because the direct hook needs V8's internal headers at build
time and private-interface upkeep on every V8 upgrade.

## Known hazard: tiering under concurrency

V8 15.8 keeps the counters that decide when to optimize a WASM function on the
shared compiled module. Many guests running the same binary at once all write
the same counters, and they slow each other down: 200 simultaneous cold
`sqlite3` starts took 32 s on V8 against 1.1 s on Wasmtime. The JavaScript
runner has the same hazard today. Starting from fully optimized code avoids
it, and `--no-wasm-dynamic-tiering` removes it at the cost of most of V8's
compute lead. A V8 patch giving each instance its own counters would keep
both; it is not part of this change.

## What this decision requires

- Replace the JavaScript runner with native imports, then delete it. Done.
- Keep isolate creation off the command's critical path. Done with a bounded
  pool of parked threads that each pre-create one isolate
  (`AGENTOS_V8_WASM_WARM_ISOLATES`, default 4). An isolate still runs exactly
  one guest; reusing isolates across guests was rejected because it would make
  one guest's leftover heap state reachable from the next.
- Handle the tiering hazard before workloads start many copies of one binary at
  once.
- Keep V8 within its support window.
- Refuse WASM features the toolchain does not use. Most recent V8 WASM bugs are
  in newer features such as garbage-collected types and exceptions.

## Results

Measured on a release sidecar (AMD EPYC 7443P, 48 threads) on November 22,
2026. "Runner" is V8 with the JavaScript WASM runner this change removed;
"native" is V8 with native host imports; Wasmtime is the opt-in executor.
Values are medians. Raw medians for every row are in
`packages/benchmarks/results/v8-native-wasm-imports-20261122.json`.

### Commands, end to end

| Workload | Runner warm | Native warm | Wasmtime warm | Runner first | Native first | Wasmtime first |
| --- | --- | --- | --- | --- | --- | --- |
| `true` | 50.5 ms | 17.0 ms | 17.1 ms | 73.4 ms | 20.3 ms | 76.7 ms |
| `ls -la`, 64 files | 175 ms | 56.6 ms | 67.3 ms | 201 ms | 103 ms | 1,195 ms |
| `sh` pipeline | 173 ms | 45.6 ms | 121 ms | 241 ms | 112 ms | 4,452 ms |
| `curl` over loopback | 189 ms | 60.1 ms | 180 ms | 290 ms | 133 ms | 6,289 ms |
| `sqlite3` sum | 104 ms | 27.8 ms | 54.7 ms | 145 ms | 52.2 ms | 1,340 ms |
| `git --version` | 94.4 ms | 39.6 ms | 133 ms | 182 ms | 93.7 ms | 3,938 ms |
| `sha256sum`, compute | 206 ms | 92.4 ms | 127 ms | 345 ms | 100 ms | 823 ms |

"First" is the first run of a command in a fresh sidecar. Wasmtime compiles
each module from scratch in a fresh process, so its first runs are slow.

### Short commands

These come from `wasm-command-floor`, which runs without the per-phase
diagnostics that `wasm-backend-comparison` enables, so the same command is
faster here than in the table above.

| Command | Runner | Native |
| --- | --- | --- |
| `true` | 25.4 ms | 14.4 ms |
| `pwd` | 37.4 ms | 16.4 ms |
| `printf`, 1 byte | 31.9 ms | 16.3 ms |
| `printf`, 64 KiB | 88.3 ms | 28.5 ms |
| `ls` of an empty directory | 57.5 ms | 20.1 ms |
| `date --version` | 50.6 ms | 24.7 ms |
| `ls` of 1,000 files | 742 ms | 350 ms |
| `echo`, warm, 10 at once | 134 ms | 33.7 ms |

### Many commands at once

Copies of `true` started together in one VM, all three engines in the same
session. Throughput counts only commands that succeeded.

| At once | Runner | Native | Wasmtime | Runner peak memory | Native peak memory | Wasmtime peak memory |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 22 per second | 47 per second | 44 per second | 158 MiB | 81 MiB | 135 MiB |
| 10 | 59 per second | 232 per second | 184 per second | 215 MiB | 92 MiB | 140 MiB |
| 50 | 99 per second | 380 per second | 268 per second | 664 MiB | 143 MiB | 151 MiB |
| 100 | 119 per second | 348 per second | 266 per second (99 of 100) | 1,334 MiB | 220 MiB | 189 MiB |

- At 200 at once every engine loses about a third of the commands to the
  VM's pipe limit (`EAGAIN: maximum pipe count reached`), so that level does
  not compare engines.
- Wasmtime drops commands above 50 at once with a mixed workload because it
  reserves each guest's maximum memory against
  `runtime.resources.maxWasmMemoryBytes`. V8 guests do not take that
  reservation (see Follow-up).

### Every WASM operation in the latency matrix

| Family | Operations | Median speedup | Range |
| --- | --- | --- | --- |
| Filesystem (`fs`) | 14 | 3.6x | 1.3x to 7.8x |
| Shell commands (`ecosystem`) | 16 | 2.9x | 1.8x to 3.7x |
| Control | 2 | 1.4x | 1.3x to 1.5x |
| Timers | 3 | 0.55x | 0.42x to 4.8x |

The JavaScript and host-native lanes of the same matrix moved by a median
1.0x, which is the noise floor. The only slower operations are short guest
sleeps: a 1 ms sleep takes about 2.2 ms because it waits on the shared
millisecond timer. Wasmtime behaves the same.

### Where a warm `true` spends its time

| Stage | Time |
| --- | --- |
| Whole command, measured by the TypeScript client | 16.3 ms |
| TypeScript client waiting for trailing output after the exit event | 10.9 ms |
| Sidecar handling the execute request | 1.5 ms |
| Sidecar exit and event bookkeeping | 1.1 ms |
| V8 engine: launch, admit the cached image, instantiate, run | 2.3 ms |

The engine is a small part of a short command. Most of it is
`SidecarKernelProxy` in `packages/core` polling twice for late output (a
0 ms turn, then a 10 ms turn) after every process with output listeners
exits. This affects every engine and every command, including JavaScript.

### What was optimized

- **Each image is admitted once per process.** Profile validation, import
  tiers, launch checks, and the compiled module are cached under a SHA-256
  digest the sidecar computes once at spawn. This cut large modules a further
  20 to 33% warm.
- **The sidecar hands the engine the bytes it already read.** The first
  native cut re-read each image through 512 KiB host calls.
- **A warm pool pre-creates isolates.** Four parked threads each hold one
  fresh isolate, saving 2 to 3 ms per command. An isolate is never reused
  across guests: reuse would let one guest's leftover state reach the next.
- **Snapshots do not apply.** A V8 startup snapshot cannot hold WASM code, and
  V8's WASM code cache needs `CompiledWasmModule::Serialize`, which agentOS's
  V8 binding does not expose. A disk cache would only help the first run after
  a sidecar restart.

## What would change it

- **Guest JavaScript loses `WebAssembly`.** Wasmtime for commands would then
  take V8's WASM engine out of a guest's reach.
- **V8 cannot be kept current.** An unpatched V8 is worse than a patched second
  engine.
- **Short commands on precompiled code dominate.** Wasmtime sets up guests
  faster and uses less memory per guest.

## Follow-up

- V8 guests do not reserve linear memory against the process-wide
  `runtime.resources.maxWasmMemoryBytes` limit, which Wasmtime guests do. Each
  V8 guest is capped at the per-VM limit and admission is bounded by executor
  slots, so the total is bounded, but not by the configured process limit.
  The JavaScript runner had the same gap.
- The fixed trailing-output wait in `SidecarKernelProxy` adds about 11 ms to
  every command. The newer client path waits for the ordered output end
  instead of polling.
- Threaded WASM runs only on the opt-in `wasmtime-threads` executor, and
  child processes inherit their parent's engine, so a threaded program started
  from a V8 shell is rejected. No shipped command uses threads today. When the
  first threaded package ships, route threaded modules to the threaded executor
  automatically, then consider native V8 threads with each thread group in its
  own process, since shared memory gives a guest a high-resolution timer.
- The Wasmtime executor pins 48.0.3, which predates the fix for a critical
  Cranelift sandbox escape on AArch64
  ([GHSA-mghf-wjfp-p5xm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm),
  fixed in 48.0.7). Move the opt-in executor to the latest 48.x release.
