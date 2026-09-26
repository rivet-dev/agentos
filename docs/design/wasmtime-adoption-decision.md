# Wasmtime adoption decision

Status: integrate Wasmtime as a selectable backend; retain V8 as the default

Date: 2026-09-26

Audience: agentOS runtime, kernel, and security maintainers

## Decision

Integrate the Wasmtime executor and engine-neutral runtime refactors from
[PR #1846](https://github.com/rivet-dev/agentos/pull/1846). Expose Wasmtime as an
explicitly selected backend while keeping V8 as the default standalone WASM
executor. JavaScript continues to require V8.

Shipping the optional backend requires a supported, patched Wasmtime pin and
resolved correctness and security findings. Changing the default is a separate
decision, gated on cold-start tails, retained memory, and representative
workload measurements. The results below explain those gates; they do not
prevent integration of the selectable backend.

[wasmtime-executor.md](./wasmtime-executor.md) records the implementation and
benchmark methodology. This note defines rollout and default-selection policy.

## Security evidence

The integrated tree pins Wasmtime **48.0.3** exactly in `Cargo.toml` and
`Cargo.lock`. Upstream [released 48.0.3 on September 24, 2026](https://github.com/bytecodealliance/wasmtime/releases/tag/v48.0.3).
Version 48 is an LTS line: under the [release policy](https://docs.wasmtime.dev/stability-release.html),
its [August 20, 2026 release](https://github.com/bytecodealliance/wasmtime/releases/tag/v48.0.0)
receives 24 months of support. Keep taking security
patches within that line; a supported major alone does not make an older patch
safe to ship.

The September 24 advisories include [Cranelift fuel-accounting amplification](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-m63x-6p34-q65x),
which affects earlier 48.x versions and is fixed in 48.0.3. This matters to our
fuel-metered Cranelift execution. The release also fixes component-model fuel
accounting and `wasmtime-wasi`/`wasmtime-wasi-http` issues. Those component and
WASI adapters are not enabled by this executor. This pin addresses the reviewed
upstream advisories as of September 26, 2026; it does not replace review of
agentOS's own host imports or future advisory monitoring.

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
  memory64 was used and relevant mitigations were disabled. The current
  Wasmtime 48.0.3 pin is outside the affected versions; this is evidence of
  failure mode, not an outstanding vulnerability in that pin.
- A separate [Winch sandbox escape](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-xx5w-cvp6-jv83)
  affected the optional baseline compiler. This branch enables Cranelift, not
  Winch, so that advisory is not directly applicable to its configuration.
- The maintainers [patched 12 advisories, including two critical ones, in
  April 2026](https://bytecodealliance.org/articles/wasmtime-security-advisories).
  Other advisories cover host denial of service and WASI permission bypasses.
  Counts cannot be compared directly with V8's because the products have
  different attack surfaces, deployments, and reporting populations.
- The reviewed baseline pinned Wasmtime 46.0.0, while
  [GHSA-2hw9-mc66-jc2q](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-2hw9-mc66-jc2q)
  identifies 46.0.0 as affected and 46.0.2 as patched. The issue requires
  particular preemption and Store mutation or reuse patterns; we have not
  established an exploit path in agentOS. The integrated tree has replaced
  that obsolete, unsupported baseline with 48.0.3; the current pin is outside
  this advisory's affected versions.

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
agentOS's host-call boundary. The optional Wasmtime backend introduces a second
compiler/runtime security boundary, configuration and advisory stream, and
adapter to maintain while V8 remains necessary for JavaScript.

## Performance and memory evidence

The [September 26 integrated-tree benchmark](../../packages/benchmarks/results/wasm-backend-comparison.json)
used source snapshot `d6591c0f41fdd19243b570edc66af1182d7a2a0c`, Wasmtime
48.0.3, and five fresh processes per backend with five samples for each of nine
workloads on one x86-64 host. Correctness passed, and the geometric-mean
workload p50 ratio was `0.236617` (Wasmtime/V8). Warm shell p50 was 48.0 ms
versus 306.8 ms; the host-call-heavy filesystem workload was 39.0 ms versus
172.2 ms. These are whole-workload timings, not isolated host-call binding
latencies.

Cold-start tails still failed: shell cold p95 was 2,657 ms on Wasmtime versus
327 ms on V8. The throughput/admission gate also failed: Wasmtime admitted
20 of 50 and 20 of 100 requested executions, while V8 admitted all 50 and
100. A faster subset is not an equal-load throughput win. At concurrency 1
and 10, both backends admitted all requested work and Wasmtime throughput
was 2.5–16.0 times V8's across the repeated/diverse rows.

Retained-memory gates passed. Median absolute process-tree RSS after VM
disposal was 523,018,240 bytes on Wasmtime versus 526,983,168 on V8; PSS was
520,773,632 versus 524,635,136 bytes. These totals include retained runtime
and compiled-module state, not only live guest memory. The older
[phase-4 artifact](../../packages/benchmarks/results/wasm-backend-comparison-phase4.json)
measured sidecar-only increases over baseline, so its memory numbers are not
directly comparable with these absolute process-tree totals.

The warm-execution gains support explicit selection, while cold tails and
admission parity still prevent a default change. Eligible copy-on-write
memory initialization is enabled; live Store/Instance snapshots, pooling,
and precompiled native artifacts are not part of this measurement.

## Gates for changing the default

Keep the optional backend on a supported, patched Wasmtime release and complete
independent review of guest-memory handling, host imports, limits, cancellation,
and Store lifetime before release. Fix confirmed process and syscall regressions
with reproducing tests.

Consider making Wasmtime the default only after repeatable benchmarks meet
cold-tail, throughput/admission, and retained-memory targets under representative
module mixes. That decision must account for the ongoing cost of securing two executor
implementations. Until these gates pass, V8 remains the default.
