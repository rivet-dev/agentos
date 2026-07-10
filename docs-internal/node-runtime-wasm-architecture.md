# Architecture handoff: Node native runtime in WebAssembly

Status: **approved direction; R0 feasibility gates required before implementation**

Decision date: 2026-07-10 (PST)

Supersedes the host-binding/libuv architecture described by revisions of
`docs-internal/node-stdlib-replacement-spec.md` before 2026-07-10. This
document is the architecture authority; the current replacement spec is its
executable acceptance plan.

This document records the architecture decision. The binding inventory,
security requirements, implementation choices, milestone plan, and measured
acceptance gates are specified in
`docs-internal/node-stdlib-replacement-spec.md`.

## Decision

AgentOS will keep the JavaScript engine native while moving Node's native
runtime below `internalBinding()` into WebAssembly:

```text
trusted host / AgentOS kernel
  ↕ only host OS-capability surface: generated AgentOS Linux/POSIX syscall ABI
existing native V8 engine
  ├─ root session isolate: pinned Node and user JavaScript
  │    ↕ isolate-local Node-API/engine calls (no OS/service capabilities)
  │    └─ V8 WebAssembly.Module / WebAssembly.Instance
  │         └─ root node-runtime.wasm instance and shared memory
  └─ bounded internal WASM-worker isolates (no user JavaScript surface)
       └─ same compiled V8 module + shared memory; wasi_thread_start only
           ├─ Node-compatible native/internal bindings
           ├─ libuv and its event loop
           ├─ OpenSSL/ncrypto
           ├─ llhttp, nghttp2, ada, c-ares, simdutf, nbytes
           └─ zlib, Brotli, zstd, and other Node native dependencies
```

V8 runs JavaScript and the root `node-runtime.wasm` instance in the existing
session isolate. Concurrent WASM pthreads use bounded internal V8 worker
isolates because a V8 isolate is single-thread-affine; they receive the same
compiled V8 module and shared `WebAssembly.Memory`, expose no user JavaScript or
public `worker_threads`, and may call only the worker-safe ABI subset. There
is no Wasmer, second Store, second compiler runtime, or cross-engine callback
bridge. AgentOS instantiates the module through V8's native
`WebAssembly.Module`/`WebAssembly.Instance` path already used by guest WASM and
Pyodide. The portable runtime follows the EdgeJS source-porting approach: port
direct-V8 Node binding code to an isolate-local Node-API import object or a
narrowly documented V8 extension instead of reimplementing each binding in
Rust or application JavaScript.

There are two interface classes but only one host OS-capability boundary.
Node-API and the narrowly documented engine extensions are synchronous,
isolate-local calls between V8 JavaScript and V8 WebAssembly. They may operate
on V8 values but cannot access OS/service capabilities. Filesystem, network, process,
thread, clock, entropy, and other OS effects cross only the generated AgentOS
Linux/POSIX syscall ABI produced from the same sysroot used by standalone WASM.
The ABI is a fixed, versioned, policy-checked import table—not unrestricted
`syscall(number, ...)` passthrough to the host kernel.

Each Node process owns one coherent runtime instance paired with one root V8
session isolate plus at most seven internal V8 WASM-worker isolates under the
frozen eight-thread aggregate cap. Its upstream libraries remain separate static archives and build
targets but link into the root `node-runtime.wasm` module so libuv, OpenSSL,
binding handles, and the allocator share one bounded linear memory. A bounded
source-built Node-API addon may load only as a validated WASM side module into
that same instance, memory, table, and accounting domain; it never creates a
second Node runtime or a host-native library.

## Current repository reality

AgentOS already compiles and instantiates WASM in the existing V8 isolate.
`crates/v8-runtime/tests/event_loop.rs` covers native asynchronous WASM
compile/instantiate and platform-task pumping;
`crates/v8-runtime/tests/node_stdlib_poc/bootstrap.js` instantiates a native
library through V8; and `crates/execution/assets/runners/wasm-runner.mjs` is the
production V8/WASI path. This program extends those mechanisms instead of
adding another engine.

AgentOS does **not** currently provide the complete isolate-local Node-API
import object required by the portable Node runtime: `napi_env`, `napi_value`,
handle scopes, references, callbacks, async work, and the V8-specific extension
surface remain implementation work. JavaScript-expressible operations belong
in a closure-private bootstrap import object; only irreducible engine hooks
belong in bounded native rusty_v8 callbacks.

The generated
`docs-internal/node-runtime-wasm-abi/agentos-node-engine-contract.json`
freezes all 85 currently imported engine extensions with their WASM signature,
result semantics, pinned EdgeJS callers, reference V8 implementation/API use,
capability family, and stable test ID. Every row currently remains
`required-unimplemented`; the inventory is a fail-closed implementation
contract, not evidence that the production rusty_v8 provider exists. Every row
forbids host OS effects.

`crates/node-api-v8` now owns the first isolate-local Node-API capability
primitive. Guest handles are globally non-reused opaque nonzero wasm32 IDs,
never host pointers; the table bounds live entries, validates kind and scope,
recursively drops nested-scope children, rejects zero/stale/cross-environment
IDs, and drops `v8::Global` values before isolate teardown. The generated 155
function Node-API provider and its callback/async/finalizer families remain R1
implementation work; this handle table alone is not a complete provider.

Other repository gaps are architecture gates, not reasons to introduce another
engine. R0 aligns rusty_v8 with V8 13.6, builds Node's OpenSSL with the owned
threaded sysroot, and proves create/join, mutex/condition/atomics, TLS
destructors, function pointers, deferred cancellation, and teardown using a
shared compiled V8 module and shared memory across internal worker isolates.
`crates/v8-runtime/src/wasm_workers.rs` now owns the production worker cap,
fixed host-thread stack reservation, rate-limited threshold warning, compiled
module reuse, structured-cloned shared backing store, V8 termination handles,
panic/error propagation, completion accounting, and deadline-bound all-worker
join barrier. The production Node bootstrap still has to connect that manager
to the shared POSIX provider; futex/poll/thread-safe-function cancellation and
addon side-module loading remain R0 gates inside the same V8 stack.

`crates/wasm-posix-host` now provides the shared Rust provider core: its
68-entry fixed syscall enum/table is generated from the checked POSIX contract,
and it enforces typed WASM signatures, bounded pending-call reservations, and
whole-range-validated copy-in/copy-out. `crates/v8-runtime/src/wasm_posix.rs`
builds the closure-private 68-function V8 import object and reads shared memory
through atomic copies from the current backing store on every call. It is not
yet R0 production wiring: the standalone and Node bootstrap paths still have
to bind this adapter to the real VM kernel dispatcher, and no release proof may
use the legacy JavaScript import assembly after that cutover.

## Boundaries and trust model

The following all belong to the same untrusted agent boundary:

- user JavaScript and npm packages;
- pinned Node JavaScript;
- `node-runtime.wasm`;
- libuv and every C/C++ dependency linked into that module.

There is no separate security boundary between user JavaScript and Node. A bug
in a Node native dependency may turn the whole Node runtime module malicious,
but it must remain unable to escape that agent's V8/WASM/kernel capabilities.

The trusted boundary is:

- native V8 and the existing rusty_v8 isolate/session integration;
- irreducible native V8 extension callbacks, if any;
- the WASM-to-POSIX syscall implementation;
- the AgentOS sidecar/kernel and its policy/resource accounting.

Every value crossing from WASM is hostile. Node-API handles must be opaque,
typed, generation-checked indexes bound to one isolate, never guest-provided
host pointers. WASM pointer/length pairs must be bounds- and overflow-checked.
Stale, forged, cross-environment, and cross-isolate handles must fail without
touching V8. A module instance is bound out-of-band to one VM/process/resource
context; no import accepts a guest-selected VM identity.

Bounds checks alone are insufficient once WASM threads share memory. Imports
copy guest-controlled descriptors into bounded host storage, validate the
copy, authorize it against the bound VM policy immediately before the side
effect, and copy results out only at the documented commit point. No import
retains a guest memory view across a yield, callback, or memory growth.

Native Node dependencies do not run in the sidecar address space. A memory
safety bug in llhttp, OpenSSL, a native addon, or a binding corrupts only that
agent's linear memory. The host must remain safe even if the entire module then
calls every available import adversarially.

## Kernel and sysroot contract

The AgentOS kernel plays Linux's role, not libuv's role. Actual libuv runs in
`node-runtime.wasm` and owns its handle lifecycle, timers, readiness decisions,
callback phases, ref/unref state, and protocol-facing queues.

Node and standalone WASM software compile against the same AgentOS-owned
sysroot and call the exact same generated POSIX syscall ABI through the exact
same policy-checked host provider. The target is native Linux behavior;
`wasm32-wasip1`, WASI preview1, and names such as `agentos_posix_v1` are only
transport. Missing libc, POSIX, threading, polling, process, signal, tty, or
filesystem behavior is fixed in the sysroot, host-import layer, or kernel. It
is not a reason to add a Node-specific host service or application patch.

The final Node path must not contain high-level `_crypto*`, `_http*`, `_tls*`,
`_zlib*`, `_tcp*`, libuv-phase, DNS, or object-shaped filesystem services. The
host exposes only the fixed standard Linux/POSIX syscall surface. The generic
engine ABI is isolate-local and must be proven incapable of OS effects.
Existing public polyfills, `internalBinding()` JS shims, and high-level bridge
services are migration inputs to delete, not target architecture.
Before the R0 feasibility gate can pass, production Node bootstrap must already
construct its V8 WebAssembly imports from the generated shared provider and
must make every legacy bridge import, dispatcher opcode, raw host-fd/path
fallback, and object-shaped host service unreachable. Legacy source files may
remain only as later deletion inventory; they are never an executable fallback.

## Event-loop integration

AgentOS must not independently reproduce libuv semantics. The existing V8
session loop drives JavaScript, the V8-managed WASM libuv reactor, platform
tasks, microtasks, and kernel readiness:

1. resume the Node runtime/libuv until immediately runnable work is exhausted;
2. obtain its readiness interests and next timer deadline through the chosen
   syscall/reactor design;
3. wait through the generic AgentOS readiness primitive without charging idle
   time as CPU;
4. resume the module with readiness;
5. let V8 synchronously cross between WASM imports/exports and JavaScript;
6. perform the existing callback-scope and microtask integration;
7. repeat while libuv reports referenced work.

The implementation spec must choose the blocking-thread, resumable import, or
equivalent mechanism. It must not recreate per-protocol queues in the host.
A bounded per-VM wake source must interrupt the generic wait for queued
foreground V8 work, thread-safe-function callbacks, cancellation, and resource
termination; otherwise the isolate can deadlock while its own thread is in
`poll`.

## Resource accounting and denial of service

The per-agent CPU account must include:

- native V8 JavaScript execution;
- Node runtime WASM execution;
- synchronous Node-API adapter work;
- synchronous syscall work and attributable asynchronous kernel work.

Waiting for I/O readiness does not count as active CPU. Limit exhaustion must
terminate the V8/WASM execution, cancel its pending kernel work, release its
resources, and leave other agents responsive.

Memory accounting includes V8 heap, WASM linear memory, Node-API handle tables,
syscall transport buffers, and kernel-owned resources. Generic VM limits remain
necessary for fds, sockets, processes, watches, pipe/socket buffers, syscall
transfer sizes, event rings, and output. Node-specific host protocol limits are
removed once the corresponding state lives in bounded WASM memory.

Like gVisor, all CPU used to service a guest must be charged to that guest. A
per-VM executor process/cgroup is the strongest implementation; a shared
sidecar requires explicit agent attribution for every synchronous and async
kernel job. No agent may create unbounded work on an unattributed shared host
thread.

The in-process design is conditional on proving V8 termination and bounded
join for every V8-WASM worker plus non-fatal handling of V8 compile/cache and
host allocation failures. If that proof fails, R0 must stop for an
architecture decision or move the entire existing V8 session behind a per-VM
executor process boundary; freeing shared memory or detaching a thread that
may still execute is forbidden.

## Startup and snapshots

Startup always uses the existing V8 snapshot and pinned WASM bytes; it may add
V8-native compiled-module or memory reuse only after the gates below pass:

1. a V8 snapshot for pinned Node JavaScript, primordials, loaders, and stable JS
   proxies;
2. pinned module bytes compiled and instantiated by V8, optionally with a
   proven V8-native compiled-module cache and clean linear-memory template.

Create a release WASM template only from a hermetic pre-tenant bootstrap that
has never received VM identity, policy, credentials, paths, time, entropy, or
user bytes. Host-owned checks, not an untrusted runtime assertion, must prove
there are no live fds, sockets, threads, timers, pending libuv requests,
addons, or persisted Node-API handles. Recreate engine handles and bind the VM
syscall context after restore. If deterministic quiescence and safe rebinding
cannot be proven, compile the pinned bytes with clean initialization and do not
ship a linear-memory template. Share compiled code across isolates only through
a supported V8/rusty_v8 contract; keep mutable memory per agent, lazy-init heavy
subsystems, and consider prewarmed pools/copy-on-write memory. Cloudflare
Pyodide is the principal reference for runtime-in-WASM memory snapshots.

## Prior art

- **Wasmer EdgeJS** is a source-porting reference, not the execution engine.
  Its architecture demonstrates the portable Node/N-API boundary, while
  AgentOS runs the resulting module in its existing V8 WebAssembly stack:
  <https://github.com/wasmerio/edgejs/blob/main/ARCHITECTURE.md>
- **EdgeJS WASIX build** is the initial implementation reference, not a drop-in
  dependency decision:
  <https://github.com/wasmerio/edgejs/tree/main/wasix>
- **Cloudflare workerd/Pyodide** proves that a substantial language runtime can
  run as WASM inside native V8 at multi-tenant scale, and documents module
  sharing, dynamic linking, linear-memory snapshots, and prewarming:
  <https://blog.cloudflare.com/python-workers/>
- **gVisor** is the resource/security analogue: a userspace application kernel
  implements Linux semantics while the whole sandbox is constrained by a
  second host boundary and cgroup. AgentOS should copy its principle that
  kernel work is charged to the guest that caused it:
  <https://gvisor.dev/docs/architecture_guide/intro/>
- **Node uvwasi** demonstrates per-instance fd/preopen/syscall contexts behind
  a shared WASI implementation:
  <https://github.com/nodejs/uvwasi>
- **Deno/workerd native bindings** are the principal alternative. They show the
  performance advantage of native host implementations and the corresponding
  cost: a larger trusted surface, native allocation/queue accounting, semantic
  reimplementation, and operation-specific DoS controls.

## High-level migration

The main implementation spec should organize the course correction around:

1. pinning and building the chosen EdgeJS/Node/libuv sources against the
   AgentOS sysroot;
2. defining the Node-API WASM wire ABI and implementing its isolate-local V8
   import object, using native rusty_v8 callbacks only where JavaScript cannot
   express a required generic engine operation;
3. binding the module to the existing VM syscall context with no Node-specific
   host services;
4. driving actual libuv from the V8 session without duplicating its semantics;
5. implementing shared CPU, memory, cancellation, and handle accounting;
6. evaluating V8-native compiled-module reuse and optional quiescent-memory
   snapshot/prewarm support without making either a correctness dependency;
7. moving one end-to-end vertical slice (`fs`, then net/HTTP, then TLS/crypto)
   through the new path;
8. deleting the corresponding public polyfills, JS binding emulation, host
   protocol state, bridge methods, queues, and Node-specific limits;
9. validating against pinned native Node tests and real-Linux fixtures.

Do not continue expanding the current host-reimplemented `internalBinding()`
or libuv event-loop path except where needed to keep the migration baseline
testable.

## Required gates for the implementation spec

The main thread must turn these into measured acceptance criteria:

- representative pinned Node compatibility for fs, loaders, async context,
  net/HTTP, TLS/crypto, compression, processes, and native Node-API addons;
- hostile Node-API and syscall import tests for forged handles, stale handles,
  pointer overflow, cross-isolate use, and teardown races;
- CPU termination of JS loops, WASM loops, crypto/KDF work, and syscall storms
  without delaying another agent;
- aggregate V8/WASM/host/kernel memory enforcement;
- cold/warm startup, any enabled compiled-module/memory reuse, small-chunk
  streaming, HTTP throughput, and crypto/compression benchmarks;
- proof that Node and standalone WASM observe the same fd, errno, readiness,
  process, socket, and filesystem semantics;
- nested V8 `WASM → import → JavaScript → Node → WASM` behavior, bounded
  termination of V8-WASM workers, and hostile addon-loader proof;
- dependency and release-artifact proof that the Node path introduces no
  Wasmer, Wasmtime, or other second WASM engine;
- a frozen per-test compatibility manifest and numeric performance/isolation
  thresholds generated before implementation results are accepted.

## Documentation cleanup required during implementation

The current stdlib replacement spec, CLAUDE files, implementation comments,
website architecture pages, resource-limit documentation, bridge contract, and
tests contain assumptions from the host-binding design. The main thread must
update or delete them as each path moves. In particular, remove claims that the
kernel plays libuv, that AgentOS must mimic libuv phases, that leaf WASM modules
are the final native-library layout, or that high-level Node bridge queues and
limits are permanent.

R0 must update the root, `crates/v8-runtime`, `crates/execution`, and
`crates/kernel` CLAUDE files with the one-host-OS-capability-boundary invariant.
Every implementation site that constructs the V8 import object, declares or
generates a syscall import, dispatches it, or authorizes its policy must carry
a nearby comment identifying the shared sysroot ABI and the controlling Linux
man page/kernel source where applicable. The machine gate must reject stale
architecture claims, any OS-capability engine import, any Node-shaped host
import, and any syscall import absent from the generated shared manifest.

Do not document this as shipped user-visible behavior until the cutover is
implemented and validated.
