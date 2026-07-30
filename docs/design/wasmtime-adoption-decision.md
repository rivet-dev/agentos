# Wasmtime adoption decision

Status: do not adopt the Wasmtime executor from PR #1846

Date: 2026-09-26

Audience: agentOS runtime, kernel, and security maintainers

## Decision

Keep V8 as the standalone WASM executor and do not merge or ship the Wasmtime
executor from [PR #1846](https://github.com/rivet-dev/agentos/pull/1846). This is
an agentOS risk and performance decision, not a claim that Wasmtime is
unfit for production generally. The PR's engine-neutral kernel, executor,
package-boundary, and embedded-VM refactors may still be valuable; review and
land those independently of the Wasmtime backend.

This note supersedes the backend-selection recommendation in
[wasmtime-executor.md](./wasmtime-executor.md). That document remains the
implementation and benchmark record for the experimental branch.

## Security evidence

Wasmtime has a serious security process: the Bytecode Alliance accepts
[private reports](https://bytecodealliance.org/security), publishes
[GitHub advisories](https://github.com/bytecodealliance/wasmtime/security/advisories),
and documents a [response runbook](https://docs.wasmtime.dev/security-vulnerability-runbook.html)
covering affected versions, CVEs, advance notice, patches, and RustSec entries.
Supported versions receive security backports. Normal versions are supported
for two months and LTS versions for 24 months, according to its
[release policy](https://docs.wasmtime.dev/stability-release.html). Fastly
[runs customer WASM on Wasmtime](https://www.fastly.com/documentation/guides/compute/getting-started-with-compute/),
so this is not an untested runtime.

The published history also contains failures at the boundary agentOS relies on:

- In April 2026, a [Cranelift miscompilation](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-jhxm-h53p-jm7w)
  could let a guest read and write arbitrary host memory on AArch64 when
  memory64 was used and relevant mitigations were disabled. The branch's
  Wasmtime 46.0.0 is newer than the affected versions; this is evidence of
  failure mode, not an outstanding vulnerability in that pin.
- A separate [Winch sandbox escape](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-xx5w-cvp6-jv83)
  affected the optional baseline compiler. This branch enables Cranelift, not
  Winch, so that advisory is not directly applicable to its configuration.
- The maintainers [patched 12 advisories, including two critical ones, in
  April 2026](https://bytecodealliance.org/articles/wasmtime-security-advisories).
  Other advisories cover host denial of service and WASI permission bypasses.
  Counts cannot be compared directly with V8's because the products have
  different attack surfaces, deployments, and reporting populations.
- This branch pins [Wasmtime 46.0.0](../../Cargo.toml), while
  [GHSA-2hw9-mc66-jc2q](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-2hw9-mc66-jc2q)
  identifies 46.0.0 as affected and 46.0.2 as patched. The issue requires
  particular preemption and Store mutation or reuse patterns; we have not
  established an exploit path in agentOS. Version 46 has also passed its
  normal support window, so it cannot be an acceptable long-lived pin.

V8 has had real vulnerabilities, including
[bugs exploited in the wild](https://chromereleases.googleblog.com/2024/05/stable-channel-update-for-desktop_13.html).
Its advantage for this decision is the much larger, longer-running exposure to
hostile code and the accompanying Chromium security program. V8's own
[security analysis](https://v8.dev/blog/sandbox) says that V8 bugs have featured
heavily in observed Chrome exploits; this is evidence of attacker scrutiny, not
proof that V8 has fewer bugs. Chromium's separate renderer-process protections
do **not** automatically come with an embedded V8 isolate. Embedders also need
to follow the active stable branch: [V8 recommends updating at least every four
weeks](https://v8.dev/docs/release-process).

Google Cloud's load-balancer plugins are not evidence for Wasmtime adoption:
Google describes a [Google-managed WASM environment](https://docs.cloud.google.com/service-extensions/docs/overview)
without naming its engine. Although Envoy contains a Wasmtime runtime,
[Envoy says it is disabled in its official build](https://www.envoyproxy.io/docs/envoy/latest/api-v3/extensions/wasm/v3/wasm.proto).

agentOS uses its own POSIX/WASI-style host imports rather than `wasmtime-wasi`.
Thus an advisory confined to `wasmtime-wasi` may not affect this executor, but
the same classes of permission and resource-limit bugs must be reviewed in
our kernel and adapters. Neither runtime's upstream security record validates
agentOS's host-call boundary. Adding Wasmtime would introduce a second
compiler/runtime security boundary, configuration and advisory stream, and
adapter to maintain while V8 remains necessary for JavaScript.

## Performance and memory evidence

The committed [phase-4 benchmark](../../packages/benchmarks/results/wasm-backend-comparison-phase4.json)
used five fresh processes per backend with five samples for each of nine
workloads on one x86-64 host. Wasmtime passed correctness, workload-median, and
throughput gates; its geometric-mean workload p50 was about 0.27 times V8's.
This is a meaningful upside, particularly for cached modules and host-call-heavy
workloads. It does not establish an isolated per-host-call binding latency win.

Cold-start tails failed the benchmark gate: for the shell workload, p95 was
4,028 ms on Wasmtime versus 481 ms on V8, dominated by compilation. The
retained process RSS *increase over baseline* after the workload corpus and VM
disposal was about 245 MiB on Wasmtime versus 154 MiB on V8 (PSS was similar).
This is **not** proof that each live
Wasmtime VM uses more memory: the measurement includes the compiled-module
cache and other process-retained state. It does show that this implementation
did not meet our measured memory target. The branch already enables eligible
copy-on-write memory initialization; it does not implement a fork of a live
Wasmtime Store/Instance. Precompilation or a faster compiler might address
cold starts, but neither has been measured here as a complete fix for retained
memory and operational complexity.

## Reconsideration criteria

Revisit this decision only with a supported, patched Wasmtime release; an
independent review of guest-memory handling, host imports, limits, cancellation,
and Store lifetime; and repeatable benchmarks that meet both cold-tail and
retained-memory targets under representative module mixes. Any reconsideration
must account for the ongoing cost of securing two executor implementations.
