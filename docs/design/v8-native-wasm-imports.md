# Native V8 WASM imports

Status: approved; replaces the JavaScript WASM runner on the V8 backend

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

- Replace the JavaScript runner with native imports, then delete it.
- Reuse isolates between guests on a fixed pool of worker threads. A new
  isolate costs about 1 ms per command.
- Handle the tiering hazard before workloads start many copies of one binary at
  once.
- Keep V8 within its support window.
- Refuse WASM features the toolchain does not use. Most recent V8 WASM bugs are
  in newer features such as garbage-collected types and exceptions.

## What would change it

- **Guest JavaScript loses `WebAssembly`.** Wasmtime for commands would then
  take V8's WASM engine out of a guest's reach.
- **V8 cannot be kept current.** An unpatched V8 is worse than a patched second
  engine.
- **Short commands on precompiled code dominate.** Wasmtime sets up guests
  faster and uses less memory per guest.

## Follow-up

The Wasmtime executor pins 48.0.3, which predates the fix for a critical
Cranelift sandbox escape on AArch64
([GHSA-mghf-wjfp-p5xm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm),
fixed in 48.0.7). Move the opt-in executor to the latest 48.x release.
