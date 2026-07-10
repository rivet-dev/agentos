# Spec: Native V8 with the Node runtime in WebAssembly

Status: **READY FOR R0 v2 — R1+ conditional on adversarial feasibility gates**

Owner: runtime team

Decision date: 2026-07-10 (PST)

Architecture authority: `docs-internal/node-runtime-wasm-architecture.md`

This spec replaces the 2026-07-09 host-binding/libuv-emulation plan. The
program starts from the `main` revision recorded in R0 evidence and forms its
own Node-only Forklift stack. It must not depend on, rebase, rewrite, or
retarget the separate curl/wget/Git networking stack. Change IDs belong in
evidence, not in this architecture contract. Historical M0/M1/M2 changes
remain migration evidence but are not ancestors of the corrected
implementation.

## 0. Decision record

The following decisions are binding unless a later measured design review
changes them.

1. **Native V8 remains the JavaScript engine.** V8 is not compiled to WASM.
   Pinned Node JavaScript runs in the existing native isolate.
2. **Each Node process owns one coherent WASM runtime instance.**
   `node-runtime.wasm` contains the EdgeJS-derived Node binding/runtime port,
   libuv, OpenSSL/ncrypto, llhttp, nghttp2, c-ares, ada, simdutf, nbytes, zlib,
   Brotli, zstd, and their allocator/runtime state. Bounded source-built addons
   are validated V8 WASM side modules in that same memory, table, instance, and
   accounting domain; each child Node process receives a distinct isolate and
   runtime instance charged to the parent VM budget.
3. **The existing V8 stack is the only JavaScript/WASM engine.** The session
   bootstrap creates `node-runtime.wasm` with V8's native
   `WebAssembly.Module` and `WebAssembly.Instance`, exactly like the existing
   guest-WASM path. There is no Wasmer, second Store, or cross-engine bridge.
   The root instance runs on the root isolate's existing dedicated session
   thread. Bounded internal V8 worker isolates execute only cloned
   `wasi_thread_start` instances over the same compiled module and shared
   memory; they expose no user JavaScript or public worker API and do not form a
   second engine/runtime.
4. **There are two interface families but one host OS-capability boundary.**
   - the versioned `agentos_napi_v1` WASM wire ABI implements Node-API v1-v10
     semantics, while `agentos_node_engine_v1` exposes the additional generic
     JavaScript-engine operations required by pinned Node within the existing
     V8 isolate. These isolate-local calls may operate on V8 values but may not
     expose OS/service capabilities;
   - WASI preview1 plus versioned AgentOS POSIX imports exposes the same fd,
     readiness, process, signal, clock, entropy, thread, and filesystem
     semantics used by standalone AgentOS WASM software. This generated,
     policy-checked import table is the only host OS-capability surface; arbitrary
     raw host syscall passthrough and Node-shaped host services are forbidden.
5. **The first reactor design uses blocking imports on a dedicated thread.**
   Actual libuv calls the generic poll/readiness import. The isolate thread may
   block while the kernel waits, but the sidecar service loop remains live and
   idle wait time is not charged as CPU. Cancellation wakes the import. This
   design is replaced only if benchmarks or a correctness test prove it cannot
   meet the gates below.
6. **EdgeJS is a pinned porting input, not a drop-in dependency.** Its portable
   runtime source and import inventory are reused; its WASIX filesystem,
   process model, V8 provider, raw-pointer bridge, compatibility stubs, and
   unpinned dependency mutations are not accepted as AgentOS architecture.
7. **Node stays pinned to v24.15.0.** Tag `v24.15.0`, commit
   `848430679556aed0bd073f2bc263331ad84fa119`, Node module ABI 137, and
   Node-API versions 1 through 10 are the compatibility target.
8. **The initial EdgeJS reference pin is**
   `b1feaa2c2b36f443ee5d527161dd93f3ac1544d6`, with reference N-API bridge
   `b3709d2506b8bfeb1cd4ede3ab737f0679378a20` and libuv-WASIX
   `cb7e09aed2fb784255d108d7c78c2063a61b3865`. The vendor manifest records
   every transitive source and patch hash. EdgeJS currently identifies itself
   as Node 24.13.2-pre; AgentOS must rebase the portable changes onto the pinned
   Node 24.15.0 sources instead of exposing that identity.
9. **Node owns one pinned OpenSSL build inside its lineage.** The source is
   OpenSSL 3.5.5 from pinned Node, built reproducibly by `toolchain/c` and
   linked into `node-runtime.wasm`. The separate registry networking stack and
   its mbedTLS backend are out of scope and remain untouched. Node must use the
   platform CA policy and prove black-box protocol interoperability, but R4
   does not migrate curl/wget/Git or require a shared TLS backend/archive.
10. **Source-built Node-API addons are conditional on an R0 loader proof.**
    Addons compiled for the AgentOS WASM target load through a bounded,
    fail-closed WASM side-module loader only after relocation, shared-memory,
    table, constructor, TLS, and unload semantics are proven.
    Prebuilt x64/arm64 ELF `.node` files fail with normal wrong-architecture
    `ERR_DLOPEN_FAILED` behavior; they are never launched on the host.
11. **Workers remain unsupported at first cutover.** libuv worker threads and
    Node-API thread-safe functions are in scope. Public `worker_threads` and
    multiple JS isolates are not.
12. **The browser runtime remains outside active CI, release, and publish
    matrices.** Its source is retained. Re-enabling it is a separate program.
13. **R0 regenerates the compatibility floor.** Historical counts are not an
    acceptance denominator because the checked-in ledger does not enumerate
    them. R0 discovers every pinned native test and commits an immutable
    per-test manifest. Every native-passing in-scope test is required; a
    required test cannot be accepted, skipped, deleted, or reclassified by the
    implementation stack.

Primary upstream authorities:

- Node-API contract:
  <https://github.com/nodejs/node/blob/848430679556aed0bd073f2bc263331ad84fa119/doc/api/n-api.md>
- EdgeJS architecture and selected source:
  <https://github.com/wasmerio/edgejs/blob/b1feaa2c2b36f443ee5d527161dd93f3ac1544d6/ARCHITECTURE.md>
- selected EdgeJS N-API bridge source reference:
  <https://github.com/wasmerio/napi/tree/b3709d2506b8bfeb1cd4ede3ab737f0679378a20>

## 1. Goal and non-goals

### Goal

Guest JavaScript observes the pinned Node 24.15.0 JavaScript and runtime
semantics while all native Node state stays inside a bounded WASM instance.
The guest cannot reach host Node APIs or select another VM's resources. Node
and standalone WASM programs observe the same AgentOS Linux/POSIX contract.

The final stack is:

```text
trusted host / AgentOS kernel and per-VM resource context
  ↕ only host OS-capability surface: generated AgentOS Linux/POSIX syscall ABI
existing native V8 isolate
  ├─ pinned Node 24.15.0 and user JavaScript
  │    ↕ isolate-local agentos_napi_v1 + agentos_node_engine_v1
  └─ V8 WebAssembly.Module / WebAssembly.Instance
       └─ node-runtime.wasm
           ├─ EdgeJS-derived Node runtime and internal bindings
           ├─ libuv
           ├─ OpenSSL/ncrypto
           └─ Node native dependencies and WASM Node-API addons
```

### Non-goals

- compiling V8 itself to WASM;
- keeping host implementations of Node module or libuv behavior;
- executing an untrusted host-native process, addon, or library;
- preserving the old bridge contract as a compatibility API;
- public `worker_threads`, inspector, or profiler support at first cutover;
- binary compatibility with prebuilt Linux native addons;
- declaring every upstream Node test portable when it requires unsupported
  workers, inspector, host build tooling, root privileges, or platform devices.

## 2. Trust boundary and invariants

User JavaScript, pinned Node JavaScript, `node-runtime.wasm`, libuv, OpenSSL,
every linked C/C++ dependency, and loaded WASM addons are one untrusted agent.
The trusted computing base is native V8, the existing rusty_v8 isolate/session
integration, irreducible native engine-extension callbacks, the POSIX host
provider, the sidecar/kernel, and resource enforcement. The closure-private
JavaScript portion of the Node-API import object is isolate-local runtime code,
not a second native security boundary.

The implementation must maintain these invariants:

1. A runtime instance is bound out-of-band to exactly one
   `{vm_id, process_id, isolate_id, generation}`. No import accepts a
   guest-selected identity.
2. Guest values are never host pointers. Every environment, value, reference,
   scope, callback, deferred, module, bytecode, and backing-store token is an
   opaque typed capability.
3. A capability contains a table kind, slot, and generation. Lookup validates
   kind, generation, environment, isolate, thread permission, and lifecycle
   state before touching V8.
4. Closing a scope or environment revokes all descendants. Generation changes
   before a slot can be reused. Generation wrap retires the slot permanently.
5. Every WASM pointer/length pair is checked against the current memory size;
   addition and multiplication use checked arithmetic; strings have explicit
   scan limits; output pointers are validated before side effects.
6. Guest linear memory is never retained as a raw host pointer across a call,
   grow, yield, callback, or thread transition. It is re-resolved through the
   current `WebAssembly.Memory.buffer` view.
7. Every import uses copy-in/validate/authorize/commit/copy-out. Nested
   descriptors, iovecs, strings, callback indexes, and options are copied once
   into bounded host storage; validation never trusts a second guest-memory
   read. Outputs and overlap are checked before side effects, and concurrent
   guest mutation cannot make host code memory-unsafe.
8. Every syscall is authorized against the out-of-band VM policy and bound
   fd/process/socket namespaces immediately before its side effect. Paths,
   symlinks, mounts, reused fds/PIDs, DNS answers, addresses, environment, and
   addon metadata cannot select or widen policy or reveal another VM's object.
9. Engine operations run only on the isolate thread. Worker threads may use
   only the explicitly thread-safe Node-API subset; other calls return the
   Node-API status required for an invalid thread/lifecycle state.
10. A callback from WASM enters through the isolate-local import adapter on
   V8's own call stack, applies Node's callback-scope and async-context rules,
   invokes JavaScript, performs the required microtask checkpoint, and closes
   scopes in all error paths. Any irreducible native extension callback creates
   the required rusty_v8 handle/callback scopes explicitly.
11. Teardown is an idempotent state machine: `running → stopping → cleanup →
   V8-dispose → WASM-dispose → dead`. Stopping rejects new user work and wakes
   waits. Cleanup keeps V8 and its WebAssembly instance alive for only the
   metered, deadline-bounded finalizer/cleanup subset. V8 references and
   backing stores are released while their WASM targets still exist; only then
   are capabilities revoked and the instance destroyed. A trap-corrupted
   runtime takes an abortive path that calls no guest finalizer, records skipped
   cleanup, and still releases every host reservation. On that path the
   environment generation is invalidated before V8 disposal; every V8-to-WASM
   callback/finalizer becomes a host-owned tombstone retained through V8
   disposal. A tombstone may release a reserved host resource exactly once but
   cannot dereference a WebAssembly instance, table, guest address, or guest
   function. Forced-GC/disposal tests cover live weak callbacks, backing
   stores, addon finalizers, and concurrent teardown at every transition.
12. A host panic, unrecoverable V8 WASM trap, V8 termination, or limit breach
   terminates only the affected VM execution and produces a typed host-visible
   error. Invalid guest input follows its generated status/errno/trap/
   termination classification and never panics, aborts the sidecar, or touches
   an unauthorized V8/kernel object.
13. Every isolate-callable host import is either in the generated shared
    Linux/POSIX syscall manifest or is an isolate-local Node-API/engine import
    proven incapable of filesystem, network, process, thread, clock, entropy,
    protocol, or other OS effects. There is no third import family.
14. Node and standalone WASM compile against the same sysroot declarations and
    libc wrappers, import the same symbol/signature for each OS operation, and
    dispatch through the same provider. A WASI or `agentos_posix_v1` transport
    name cannot change semantics, authorization, errno, or accounting.
15. The provider exposes a fixed generated syscall table, not unrestricted
    guest-selected raw host syscall numbers. Every call is typed, validated,
    authorized against the bound VM, accounted, and tested against Linux.

Security tests use a purpose-built hostile WASM module, not only the trusted
Node runtime. At minimum they cover forged kinds, zero/maximum IDs, stale
generations, cross-env and cross-isolate handles, pointer wrap, out-of-bounds
reads and writes, memory growth races, callbacks during teardown, double close,
wrong-thread calls, callback reentrancy, policy-bypass attempts, symlink/rename
races, fd/PID reuse, cross-mount and cross-VM access, socket/DNS allowlists,
Unix sockets, signals, and inherited credentials. The generated ABI inventory
classifies every invalid input as a returned status/errno, a guest trap, or VM
termination; tests reject classification drift.

Every Node-API, engine-extension, POSIX, reactor, and addon-loader ABI row
records valid lifecycle/thread states, hostile pointer/nested-memory/capability
cases, authorization decision, side-effect commit point, reservation delta,
cancellation result, expected status/errno/trap/termination, cross-VM
assertion, and concrete test IDs. The machine gate fails any importable row or
lifecycle transition without this coverage or with a stale/missing test ID.

## 3. Source, vendoring, and build

### 3.1 Repository ownership

- `crates/node-stdlib`: pinned Node JavaScript, fixtures, compatibility
  ledgers, and the dual-flavor harness retained from M0.
- `crates/node-runtime-wasm`: pinned EdgeJS-derived portable runtime sources,
  patch series, build orchestration, ABI manifests, and the final WASM artifact.
- `crates/node-api-v8`: the isolate-local Node-API import adapter, irreducible
  native V8 extension callbacks, and hostile import tests.
- `crates/wasm-posix-host`: the reusable AgentOS WASI/POSIX import provider.
  Standalone WASM and the Node runtime must share its kernel-facing behavior.
- `crates/v8-runtime`: one-isolate orchestration, snapshots, V8 scopes,
  microtasks, termination, and coordination with the Node runtime. It does not
  own libuv phases or Node protocol queues.
- `toolchain/c`: AgentOS sysroot/libc, compiler wrappers, Node's pinned OpenSSL,
  and other reusable native build outputs.
- `crates/kernel`: fd/process/socket/readiness state, permissions, limits, and
  cancellation.

Names may change during implementation, but these ownership boundaries may
not.

### 3.2 Vendor rules

The vendor script records tag/commit, archive URL, archive SHA-256, submodule
commit, every included file hash, applied patch hashes, compiler identity,
sysroot hash, configure flags, and final artifact hash. Check mode must
reproduce the source tree without network access.

Porting patches live under
`crates/node-runtime-wasm/vendor/patches/{edgejs,node,libuv,...}`. Each patch
states:

- the upstream commit and file it derives from;
- why the change cannot live in the sysroot, generic host ABI, or kernel;
- the upstream issue/PR when one exists;
- the conformance test that prevents accidental divergence.

No build step mutates a checkout or downloads an unpinned dependency. R0 runs
two clean offline builds in the pinned build image and requires identical
source-tree, archive, and final-module hashes. The manifest includes tool
binary digests; deterministic archives, path remapping, locale, timezone, and
`SOURCE_DATE_EPOCH` are fixed.

### 3.3 Toolchain and output

The module is built by the repository's normal build pipeline against the
AgentOS-owned sysroot. EdgeJS's WASIX compiler flags and dependency patches are
an inventory, not the final toolchain. Missing libc/POSIX behavior is added to
the AgentOS sysroot and generic import provider.

The final link emits one module with:

- imported bounded shared memory when threads are enabled;
- an explicit maximum memory;
- a bounded table;
- stack guards;
- WASM exceptions only if the pinned V8 WebAssembly engine and generated code
  prove correct teardown under traps;
- no undefined import outside the two versioned ABI manifests;
- exports for runtime create/bootstrap/run/interrupt/teardown, allocator
  integration, quiescence inspection, and snapshot restore.

The root module and every addon side module have separate signed manifests.
Addon bytes are hostile: section counts and sizes, imports, signatures,
memories, tables, relocations, constructors/destructors, TLS, compilation
CPU/time, and cache admission are bounded and validated before instantiation.
All transitive imports must match the versioned engine/POSIX manifests.
Host-native objects and unbounded modules fail closed, and tenant addons can
neither populate nor evict the trusted runtime cache.

CI verifies the import/export manifest and fails on drift. Release staging
records the source manifest, sysroot hash, V8/rusty_v8 version and WebAssembly
feature flags, module hash, initial/maximum pages, table maximum, and Node
OpenSSL archive hash.

## 4. V8 WebAssembly and reactor integration

### 4.1 Existing V8 execution path

The engine choice is already made: the existing `crates/v8-runtime` isolate
compiles and instantiates `node-runtime.wasm` through V8's native WebAssembly
API. R0 extends and productionizes the current `WebAssembly.Module`,
`WebAssembly.Instance`, async compile/instantiate, snapshot-restore, and
WASI-runner tests. It does not add an engine dependency.

R0 records the pinned V8/rusty_v8 build and proves the required V8 WebAssembly
features:

- Node runtime compile success, WASM exceptions, threads, SIMD, and atomics;
- isolate termination of JavaScript and WASM execution, including nested
  imports/exports;
- explicit linear-memory/table maxima and aggregate accounting;
- safe reuse of compiled modules only if V8/rusty_v8 exposes a supported,
  version-checked path;
- cold compile, cached load, bootstrap, fs, HTTP, crypto, and compression;
- binary size and resident memory.

Each Node process receives one root V8 isolate/context, one root WebAssembly
instance, its own memory/tables, closure-private Node-API handle tables,
syscall context, and limits. Nested `WASM export → import function → JavaScript
callback → Node builtin → WASM export` stays on V8's supported call stack. R0
tests nested success, exceptions, termination, traps, memory growth, and
teardown; it does not invent a cross-engine trampoline.

The native V8 build is part of the Node pin. R0 must either align rusty_v8/V8
with Node 24.15.0's V8 `13.6.233.17`, or commit a reviewed API/behavior delta
covering every engine extension, JavaScript semantic, ICU/data dependency,
snapshot format, cached-data format, and affected Node test. Cached data or a
snapshot produced by one V8 revision is never consumed by another.

### 4.2 Blocking reactor

The main runtime invocation runs on the existing dedicated V8 session thread:

1. pinned Node JavaScript calls its closure-private `internalBinding()` entry.
2. that adapter invokes a typed export on the V8-managed WASM instance.
3. the runtime invokes isolate-local Node-API import functions to
   create/read/call V8 values.
4. libuv runs immediately ready work.
5. When libuv has no ready work, it invokes the generic poll syscall with its
   fd interests and timer deadline.
6. The import submits one bounded kernel wait associated with the VM and blocks
   the session thread.
7. Readiness, deadline, signal, cancellation, or resource termination wakes
   the import.
8. libuv consumes the result and invokes JavaScript callbacks through Node-API.
9. the existing V8 session performs callback-scope, platform-task, and
   microtask integration.
10. The loop continues while libuv reports referenced work.

The sidecar service loop and other VMs never wait on this thread. No host queue
represents timers, immediates, next ticks, TCP state, HTTP state, TLS state, or
libuv handles.

The poll import validates a bounded subscription array, copies it once, and
returns a bounded event array. Cancellation is level-triggered and idempotent.
The kernel wait owns no borrowed WASM memory. Linux `poll(2)`, `epoll(7)`,
`eventfd(2)`, and libuv's pinned backend are cited at the implementation and
fixture sites.

A bounded per-VM wake source is always part of the wait. Foreground V8 tasks,
thread-safe-function callbacks, async-work completion, session shutdown,
runtime control messages, cancellation, and budget termination signal it.
The isolate drains the bounded engine-callback queue before blocking again;
no producer may rely on the blocked isolate thread to wake itself.

Queue publication and wait arming use a generation-checked
publish-then-signal/arm-then-recheck protocol. R0 deterministically exercises
enqueue before, during, and after wait arming, coalesced signals,
stale-generation signals, cancellation, and teardown; no interleaving may lose
a wake or create an idle busy loop.

### 4.3 Threads

The threaded ABI freezes only after the existing V8 stack runs a
production-toolchain probe using V8 shared `WebAssembly.Memory`, worker
contexts/instances, the versioned POSIX manifest, futexes,
mutexes/condition variables, TLS destructors, function pointers, traps,
cancellation, and bounded stacks. The design records whether tables are shared,
replicated at identical indices, or synchronized on loader updates; it does not
assume table sharing. Defaults are bounded; the initial runtime thread cap is 8
including the main thread, with warning at 80 percent and a typed error naming
`limits.nodeRuntime.maxThreads`.

V8 isolates are thread-affine. The root session isolate continues to own all
Node/user JavaScript and engine imports; each concurrent pthread therefore uses
one bounded internal V8 WASM-worker isolate reconstructed from V8's supported
`CompiledWasmModule` sharing and structured-cloned shared-memory backing store.
The worker executes only `wasi_thread_start`, receives no user global/context,
and rejects isolate-local engine operations except the explicitly thread-safe
Node-API subset. This is multiple isolates inside the one existing V8 engine,
not a second Store or a second runtime instance.

Thread creation reserves stack and aggregate VM memory before launch. Every
thread is attributed to the VM for CPU and cancellation. On teardown, futex
waits wake, new work is rejected, threads join within a bounded grace period,
and failure to quiesce terminates the instance. A worker cannot access rusty_v8
except through the standard thread-safe-function path, whose callback is queued
to and drained by the isolate thread.

Every V8-WASM worker has a host-owned termination handle and joins a termination
barrier. No isolate/context, instance, memory, table, capability table, or
syscall context is freed until every worker has exited host imports and
acknowledged termination. R0 proves bounded termination of compute, futex,
poll, thread-safe-function, and injected-stuck paths. If the existing V8 stack
cannot guarantee this in-process, the entire V8 session moves behind a per-VM
executor process boundary; force-detaching a worker or freeing live shared
memory is forbidden.

## 5. Node-API and engine extension ABI

### 5.1 Standard Node-API

The provider implements the complete stable Node-API v1-v10 surface from the
pinned Node headers, not only the subset imported by the runtime. The generated
inventory is checked into the repository with:

- import name and WASM signature;
- Node-API version;
- guest pointer/handle fields;
- thread and lifecycle requirements;
- V8 operation used;
- tests and current status.

Node-API defines C semantics, not a standard WASM wire ABI. R0 therefore
generates and freezes the `agentos_napi_v1` guest header and manifest, including
wasm32 little-endian scalar layouts, handle widths, pointer/length rules,
callback signatures, status values, ownership, and copy-in/copy-out commit
points. ABI compatibility is checked from C against the isolate-local import
adapter and any native Rust extension callbacks; no host pointer or native V8
representation appears in the guest ABI.

Node's `node_api.h`, `js_native_api.h`, and official Node-API documentation
are the contract. All functions return `napi_status`; values use out
parameters; pending exceptions and last-error state are environment-local.

The first complete provider includes values and coercion, strings, BigInt,
objects/properties, functions/callback info, classes, errors/exceptions,
ArrayBuffer/typed arrays/DataView/Buffer, promises/deferreds, references,
handle scopes, wraps/finalizers, type tags, instance data, cleanup hooks,
async contexts/work, callback scopes, thread-safe functions, and external
memory accounting.

### 5.2 Engine extensions

Node internals still require V8 capabilities that stable Node-API does not
express. They live in one versioned `agentos_node_engine_v1` manifest. Allowed
families are:

- context creation, contextify, script/function compilation, cached data;
- ES module creation, linking, instantiation, evaluation, namespaces, dynamic
  import, and import-meta callbacks;
- promise hooks/details and continuation-preserved embedder data;
- microtask enqueue/checkpoint and foreground-task scheduling;
- structured clone and transfer;
- V8 type/proxy/constructor/property inspection required by pinned Node;
- stack traces, error source positions, and source-map hooks;
- heap statistics, low-memory notification, and termination;
- runtime callback/cleanup hooks needed to integrate the isolate.

Every extension row cites the pinned Node or EdgeJS caller and the rusty_v8/V8
API used. An extension is rejected if it implements Node module, libuv,
filesystem, network, crypto, compression, process, or protocol behavior.
Extension ABI drift increments the namespace version rather than silently
changing a signature.

### 5.3 Buffers and backing stores

There is no permanent raw sharing of arbitrary guest memory with V8. The
provider supports two explicit modes:

1. copy mode for normal strings and bounded transfers;
2. pinned external backing mode for Buffer/ArrayBuffer, where a host-owned
   reservation binds a checked guest allocation to a V8 BackingStore for a
   bounded lifetime.

Pinned backing records carry env/isolate/generation/offset/length, prevent
memory relocation or incompatible grow, charge both logical and resident
memory once, and release through a finalizer safe during teardown. R0 measures
copy versus pinned mode; correctness, GC/finalizer safety, and aggregate
accounting gate use of the zero-copy path.

Copy mode is the required cutover path. Pinned external backing remains
disabled unless R0 proves stable addressing across V8 memory growth, shared-memory
mutation, nested callbacks, GC, finalization, and teardown. Failure of that
optimization proof does not permit unsafe zero-copy or block the architecture.

### 5.4 Addon side modules

R0 must load one separately compiled C/C++ Node-API addon and prove bounded
relocation, symbol resolution, constructors/destructors, memory/table updates,
function pointers, TLS for existing and future workers, callbacks, unload,
stale-handle rejection, and repeated load/unload without leakage. Current
`dlopen`/`dlsym` exclusions and non-PIC/no-DSO archives are explicit gaps, not
evidence. If the guest loader proof fails, source-built addons are declared
unsupported at first cutover through an architecture decision; host-native
loading and fake success shims are forbidden.

## 6. POSIX, sysroot, and kernel contract

The AgentOS kernel plays Linux's role. The WASM host provider does not play
libuv and exposes no Node-shaped operations. It is the only host OS-capability
surface reachable from the V8 isolate. Node-API and engine imports stay inside
the isolate and cannot perform OS effects.

The required shared contract includes:

- fd allocation, duplication, flags, stat/statfs, directory iteration, links,
  rename, chmod/chown, timestamps, mmap-like allocation where required, and
  inotify-compatible watches;
- TCP, UDP, Unix sockets, DNS transport, socket options, shutdown, accept,
  nonblocking I/O, readiness, and correct errno;
- pipes, PTYs, process spawn/wait, process groups, signals, and terminal ioctls;
- monotonic/realtime/process/thread clocks, timers, entropy, identity, limits,
  and resource usage;
- pthread creation/join/TLS, mutexes/condition variables, futex wait/wake, and
  libuv thread-pool support;
- cancellation that wakes every blocking call without inventing a successful
  result.

R0 generates one syscall manifest from the AgentOS sysroot declarations. It
records every import module/name, exact signature, libc entry point, Linux/POSIX
authority, authorization rule, errno mapping, accounting class, bound, and
conformance test. Where preview1 lacks an API, the patched libc calls a
versioned `agentos_posix_v1` import in that manifest. This is a transport ABI,
not permission for a second behavior or provider. Unrestricted
`syscall(number, ...)`, Node-module operations, libuv phases, protocols, and
object-shaped host services are forbidden.

At the import declaration/generator, V8 import-object construction, provider
dispatch, and policy-authorization sites, implementation comments identify the
shared sysroot declaration and cite the controlling man page, Linux kernel
source path, RFC, or consumer parser. Tests name a captured real-Linux fixture.
A missing syscall is implementation work, never a reason for a Node-specific
bridge.

R0 moves production V8 WebAssembly import-object construction onto the shared
generated Rust provider. The Node flavor must have zero reachable legacy
imports, string-dispatched bridge opcodes, raw host-fd fallbacks, direct host
paths, or object-shaped host services before R0 exits. The standalone WASM
runner and Node runtime call that same provider in R0; unused legacy files may
remain as deletion inventory, but no R0 proof may execute them. A cross-runtime
conformance suite proves identical fd numbering rules, errno, readiness, stat
fields, socket behavior, process behavior, and filesystem visibility before
the duplicate files are deleted.

## 7. Resource accounting and limits

### 7.1 Aggregate budgets

One VM budget covers:

- V8 heap and external memory;
- V8 WebAssembly linear memory, tables, stacks, compiled code, and instance
  state;
- Node-API capability tables and pinned backing stores;
- syscall copies and pending kernel work;
- fds, sockets, processes, watches, pipes, PTYs, and buffers;
- CPU consumed by V8, WASM, synchronous imports, and attributable kernel work.

Subsystem limits are reservations beneath the aggregate limit, not independent
allowances that may all be consumed simultaneously. Reservation failure is
atomic. Deallocation returns the reservation even during trap/teardown paths.

### 7.2 CPU and cancellation

One atomic per-VM CPU ledger accounts for the main V8 session, every V8-WASM
worker, synchronous imports, async/kernel jobs, addon compilation,
cancellation, and cleanup. Existing V8 execution termination, the typed WASM
execution deadline currently carried by `max_fuel`, and stack guards are
calibrated against that same remaining budget and extended to every worker. R0
must rename or precisely define that field's time units; this spec does not
claim V8 provides instruction fuel. Blocking readiness time does not debit CPU,
but CPU used to enter, wake, copy, validate, or cancel the wait does.

Limit exhaustion:

1. marks the VM terminating;
2. terminates V8 execution;
3. terminates every V8-WASM worker and traps outstanding WASM calls;
4. cancels and wakes kernel work;
5. rejects new callbacks and syscalls;
6. tears down resources; and
7. returns one typed error identifying the exhausted field and configured
   value.

Tests cover JS loops, WASM loops, OpenSSL KDF/crypto, compression, syscall
storms, and async kernel work while a second VM continues meeting the numeric
p99 latency bound frozen in R0.

### 7.3 Bounded defaults

R0 may tune these values upward with measured evidence; it may not replace a
bound with infinity.

| Resource | Initial default | Typed field |
|---|---:|---|
| aggregate VM memory (V8/WASM/host/kernel) | 384 MiB | `limits.vm.maxMemoryBytes` |
| WASM linear memory within aggregate | 256 MiB | `limits.nodeRuntime.maxLinearMemoryBytes` |
| runtime threads including main | 8 | `limits.nodeRuntime.maxThreads` |
| live Node-API values | 65,536 | `limits.nodeRuntime.maxNapiValues` |
| refs/deferreds/scopes/callback records, each | 16,384 | `maxNapiRefs` / `maxNapiDeferreds` / `maxNapiScopes` / `maxNapiCallbackRecords` |
| pending syscalls | 4,096 | `limits.nodeRuntime.maxPendingSyscalls` |
| pending engine callbacks/tasks | 16,384 | `limits.nodeRuntime.maxPendingCallbacks` |
| poll subscriptions or returned events | 4,096 | `limits.nodeRuntime.maxPollEvents` |
| one import transfer | 16 MiB | `limits.nodeRuntime.maxTransferBytes` |
| WASM table entries | 65,536 | `limits.nodeRuntime.maxTableEntries` |
| loaded WASM addons | 64 | `limits.nodeRuntime.maxAddons` |
| one addon module | 64 MiB | `limits.nodeRuntime.maxAddonBytes` |
| teardown grace period | 5,000 ms | `limits.nodeRuntime.maxTeardownGraceMs` |
| process-wide compiled-module cache, if enabled | 8 entries / 512 MiB | operator runtime setting |

R0 generates `docs-internal/node-runtime-wasm-limits.json` for every
guest-controlled allocation, queue, wait, cache, scan, compiler job, and
teardown phase. Each row has a finite default and hard maximum, rate-limited
80-percent warning, typed field/error, reservation point, cleanup rule, and
saturation test; the checker fails an unlisted resource. Allocation fails
before side effects and the error names how to raise the limit. Guest code
cannot alter hard caps with environment variables; trusted typed VM
configuration owns them.

## 8. Startup, snapshots, and packaging

Startup always uses the first two independently versioned artifacts and may
add the third only after its safety gates pass:

1. a V8 snapshot containing pinned Node JavaScript, primordials, loaders, and
   stable host proxies;
2. pinned `node-runtime.wasm` bytes, with an optional V8-native compiled-module
   cache only if the pinned rusty_v8 API supports it safely;
3. an optional initialized linear-memory template.

Release memory templates are produced only by a hermetic pre-tenant bootstrap
that has never received a VM identity, policy, credential, environment, path,
clock sample, entropy, fd, or user byte. The runtime quiescence export is
advisory; host-owned checks must independently prove:

- no open fd/socket/process/PTY/watch;
- no live libuv handle/request/timer/thread;
- no pending syscall, callback, cleanup hook, or addon;
- no Node-API handle/reference;
- no agent entropy, time, environment, path, credential, or VM identity;
- only immutable runtime tables and allocator state remain.

Restore creates a fresh V8 context, WebAssembly instance/memory, Node-API
environment and generation, binds the VM syscall context, seeds entropy,
reconstructs engine handles, and then admits user code. A template hash
includes the Node, EdgeJS port, sysroot, OpenSSL, ABI, V8/rusty_v8, compiler,
and module hashes.

The initial template is pre-environment unless a paired V8/WASM reconstruction
manifest proves every guest ID can be rebound without persisting a host
capability, V8 handle, kernel object, raw pointer, or tenant input. Two clean
builds must produce byte-identical eligible state except explicitly regenerated
fields. Canary-secret, corrupt-artifact, stale-version, cross-VM, and
cross-architecture tests must fail closed. Guest-supplied snapshots or compiled
artifacts are never deserialized. If these gates fail, release uses only the
compiled-module cache and clean initialization.

Compiled WASM code may be shared only through a supported V8/rusty_v8 contract
when V8 version, host architecture, CPU features, compiler flags, and module
hash all match.
Mutable memory, tables, capability state, and kernel handles are never shared
between agents.

Cold start, warm start, snapshot creation, snapshot restore, template RSS,
copy-on-write behavior, and first-use latency for crypto/HTTP/addons are
measured. A restore-equivalence suite compares clean boot and restored boot
including entropy uniqueness and teardown.

## 9. Compatibility, tests, and performance

### 9.1 Harness policy

R0 commits `docs-internal/node-runtime-wasm-compatibility.json`. It records the
frozen discovery command and hash plus every pinned native test's exact ID,
family, native result, in-scope/required bit, and exclusion reason. Every
native-passing in-scope test is required. At R7 there are zero accepted,
skipped, missing, deleted, or weakened required tests. Changing required to
excluded, deleting an identity, weakening an assertion, or accepting a former
pass requires a separate architecture decision, not an ordinary ledger edit.

Every discovered native-passing test defaults to required. Exclusion requires
a machine-readable code mapped to an enumerated non-goal in section 1 plus
detected dependency proof. The checker rejects unknown codes, exclusions in a
required family from section 9.2, missing/duplicate/unmapped IDs, and any
discovery-count reduction. Evidence reports counts and set differences by
family so aggregate totals cannot hide a regression.

The existing M0 runner and ledgers remain migration inputs, but their aggregate
counts are not the completion denominator until R0 regenerates and checks this
per-test manifest.

The suite executes:

- native pinned Node on Linux;
- the frozen legacy flavor until deletion;
- the new WASM-runtime flavor.

Comparisons use exact test identities, exit status, stdout/stderr, errno,
ordering, and timeout. Tests that require unsupported workers/inspector or
nonportable host setup remain explicit skips, not denominator removal.

### 9.2 Required conformance families

- bootstrap, globals, process identity, CJS, ESM, package exports/imports;
- Buffer, encoding, URL, events, streams, timers, async hooks/context;
- fs sync/async/streams/watch and real-Linux stat/errno fixtures;
- TCP, UDP, Unix sockets, DNS, HTTP/1, HTTP/2, undici/fetch, backpressure;
- TLS 1.2/1.3, certificates, SNI, ALPN, crypto, WebCrypto, KDFs;
- zlib, Brotli, zstd, llhttp, ada, c-ares, nghttp2;
- child process, pipes, stdio, PTY, signals, exit/beforeExit;
- when enabled by R0, source-built C and C++ Node-API addons covering Node-API
  versions 1, 8, and 10, async work, thread-safe functions, Buffer, finalizers,
  cleanup, and addon unload; otherwise wrong-architecture/unsupported
  `ERR_DLOPEN_FAILED` behavior and documentation;
- hostile Node-API/syscall modules and teardown races;
- cross-runtime POSIX fixtures shared with standalone WASM.

### 9.3 Performance gates

All benchmark artifacts record hardware, kernel, build mode, compiler/backend,
module hash, sample count, warmup, p50, p95, p99, IQR, min, and max. Required
rows are:

- cold compile, clean bootstrap, and any enabled cached-module/template path;
- CJS/ESM import storms;
- 4 KiB and 1 MiB fs reads/writes, stat/readdir storms, stream copies;
- 1-byte and 16 KiB TCP/HTTP streaming, request throughput, backpressure;
- TLS handshake/resumption and small-record throughput;
- SHA/AES/RSA/KDF and zlib/Brotli/zstd;
- Node-API scalar calls, property access, callbacks, Buffer copy/pinned paths;
- idle runtime RSS and aggregate RSS under concurrent VMs.

Every required benchmark is a three-way before/after comparison run with the
same fixture, input, host, build profile, warmup, sample count, and statistic:

1. pinned upstream Node 24.15.0 running natively on Linux (`nativeNode`);
2. the frozen pre-refactor AgentOS Node implementation (`legacyAgentos`); and
3. the new Node-in-WASM implementation in the existing V8 stack
   (`nodeRuntimeWasm`).

The machine-readable row and human report show the absolute result for all
three implementations plus the new implementation's numeric delta and ratio
against both native Node and the legacy implementation. A missing comparator,
an incomparable workload, or a result reported only as a percentage fails the
gate. The legacy executable and fixture inputs are frozen in R0 and retained as
benchmark-only release evidence after the implementation itself is deleted;
they are never reachable from the shipped runtime after R7.

Before implementation measurements, R0 commits
`docs-internal/node-runtime-wasm-performance.json` with the exact command,
fixture, host/build profile, samples, statistic, native/legacy baseline, noise
rule, and numeric threshold for every row. Each row has explicit
`nativeNode`, `legacyAgentos`, and `nodeRuntimeWasm` result slots plus computed
new-versus-native and new-versus-legacy deltas. It includes numeric offender
termination and control-VM p99 latency ceilings; under hostile concurrency the
control VM must also remain at or below twice its unloaded p99. Missing or
post-hoc thresholds fail the gate.

Cutover requires no unapproved regression greater than 10 percent against the
legacy M0 supported behavior on the same host/build protocol. Performance
improvements do not compensate for a correctness regression. Any approved
exception names the benchmark, cause, owner, expiry milestone, and evidence.
No performance exception survives R7.

## 10. Migration and deletion rules

The new runtime is selected by an internal flavor until the cutover gate. The
legacy bridge may receive only correctness/security fixes needed to keep the
baseline runnable. No new public Node behavior lands there.

Migration proceeds by vertical slice. A slice is complete only when:

1. pinned Node JavaScript uses the WASM runtime path;
2. Node/libuv calls generic POSIX imports;
3. native and AgentOS conformance tests pass;
4. limits and hostile tests pass;
5. performance evidence is recorded; and
6. the replaced JS shim, host binding, bridge contract entry, queue/state,
   Node-specific limit, tests, and docs are deleted in the same or immediately
   following focused revision.

Final deletion includes:

- public builtin/polyfill implementations under `packages/build-tools`;
- `internalBinding()` JS emulation in `crates/node-stdlib/adapter`;
- high-level `_fs*`, `_tcp*`, `_http*`, `_tls*`, `_crypto*`,
  `_zlib*`, timer-phase, and handle-registry bridge methods;
- sidecar Node protocol state and operation-specific queues;
- RustCrypto Node crypto and emulated OpenSSL identity;
- separate leaf-WASM loaders superseded by the coherent runtime;
- obsolete Node-only resource-limit fields;
- the real/legacy flavor flag and frozen legacy implementation.

The generic syscall provider, kernel primitives, Node-API provider, V8 engine
extensions, compatibility harness, and Node OpenSSL/toolchain remain.

Before Node implementation, R0 commits
`docs-internal/node-runtime-wasm-networking-isolation.json` with the immutable
`main` baseline commit, exact curl/wget/Git/SSH/mbedTLS source, vendor, lock,
build, and test globs, and each baseline file hash. The manifest cannot be
regenerated after R0. The Node candidate diff from that recorded main commit
must be empty for every protected glob at R0 and every later milestone;
pre-existing duplicated changes must be split out before R0 can pass.
Black-box tests consume published networking artifacts by digest only. The
gate also requires a Forklift dry run proving no networking PR was retargeted
or reparented.

R0 also commits `docs-internal/node-runtime-wasm-forbidden.json`, covering
legacy symbols, paths, ABI imports/exports, bridge opcodes, dependencies,
flags, generated output, binaries, packages, and documentation claims. Every
temporary file-only match names its deleting milestone. Entries cannot be
renamed or removed without deletion proof; R7 requires an empty allowlist and
zero matches. Reachable imports, dispatch opcodes, raw host-fd/path fallbacks,
and host OS effects have no temporary allowlist in the Node flavor: R0 requires
zero such matches in the generated import graph and production bootstrap trace.

Completion is machine-checkable. `scripts/check-node-runtime-wasm-gates` must
fail closed unless one immutable implementation revision `C` plus its
evidence-only child attestation `E` reports R0-R7 green with zero
TBD/TODO items, missing ABI/limit rows, unbounded resources, forbidden legacy
symbols, required compatibility failures, unapproved benchmark regressions,
networking-path drift, or absent/stale evidence. “Mostly passing,” inspected
logs, accepted required failures, or deferred follow-ups do not pass.

Documentation and implementation-site guidance are part of the gate, not R7
cleanup. R0 updates this spec, the architecture authority, and the root,
`crates/v8-runtime`, `crates/execution`, and `crates/kernel` CLAUDE files to say:
one existing V8 engine with one root session isolate and bounded internal WASM
worker isolates; root-isolate engine calls; one shared Linux/POSIX
host OS-capability ABI. Each milestone updates those documents when the contract
changes. The checker rejects stale second-engine/host-binding claims, any
OS-capability engine import, any Node-shaped host import, any syscall absent
from the shared generated manifest, and missing implementation-site comments
required by section 6.

## 11. Milestones

Every milestone begins in a fresh implementation revision `C`, then ends in an
evidence-only child attestation `E`. Both receive conventional descriptions,
pass leakage/audit checks, and are submitted with Forklift before the next
milestone begins.

### R0 — feasibility, pins, and ABI freeze

Deliver:

- vendor/check scripts and manifests for EdgeJS, the N-API reference, libuv,
  pinned V8/rusty_v8, and all native dependencies;
- an EdgeJS-to-Node-24.15.0 source/behavior delta report;
- a Node-V8-to-rusty_v8/ICU version decision and generated API/behavior delta;
- reproducible `node-runtime.wasm` build against the AgentOS sysroot;
- generated Node-API wire, engine-extension, POSIX import, and export
  inventories;
- generated shared-sysroot syscall manifest plus a scan proving that it is the
  only host OS-capability import surface and that engine imports have no OS
  effects;
- shared generated-provider wiring for production V8 WebAssembly instances,
  with zero reachable legacy imports, raw host-fd/path fallbacks, or
  object-shaped host services in the Node flavor;
- V8 WebAssembly feature, limit, compile/instantiate, and benchmark report
  against the existing standalone-WASM path;
- production bootstrap proof that the existing V8 session loads the pinned
  module bytes and instantiates `node-runtime.wasm` with closure-private import
  objects, without launching host Node or another engine;
- the nested same-isolate call/exception/termination proof from section 4.1;
- a production-toolchain pthread/libuv-worker probe covering create/join,
  mutex/condition/futex, TLS, function pointers, cancellation, teardown, and
  aggregate accounting, plus threaded OpenSSL using the intended archive;
- the addon-loader proof from section 5.4;
- one generic fd/read/write/poll round trip through the VM kernel context;
- hostile handle/pointer smoke tests and a written threat model;
- generated compatibility, limits, performance, ABI-result-classification,
  and networking-path baseline manifests;
- dependency, binary, and release-artifact scans proving the Node path adds no
  Wasmer, Wasmtime, or other second WASM engine;
- V8 heap OOM, WebAssembly compile/instantiate/JIT failure, Rust panic,
  corrupt-cache, and stuck-worker fault injection;
- proof that M0's harness still runs as migration evidence.

Exit gate: no raw host pointer crosses the WASM ABI; the module builds
reproducibly and no second WASM engine is linked or shipped; the existing V8
engine supports the required threads, atomics, exceptions if used,
deadline/termination interruption, nested calls, and bounded
all-worker termination; compiled-module reuse is either proven through a
supported V8 API or explicitly disabled; addon support has an explicit pass or
architecture decision; every threshold is numeric and frozen; another VM
meets the recorded p99 ceiling during a blocked poll, WASM loop, and injected
failure. Any failed mandatory go/no-go item, or addon failure without the
committed scope decision from section 5.4, stops before R1.

### R1 — complete V8 import adapter and runtime lifecycle

Deliver:

- required Node-API v1-v10 provider;
- `agentos_node_engine_v1` for bootstrap, contexts/modules, promises,
  microtasks, async context, and termination;
- bounded typed capability tables, Buffer/backing-store modes, finalizers,
  cleanup hooks, callbacks, async work, and thread-safe functions;
- deterministic create/bootstrap/run/interrupt/teardown lifecycle;
- pinned Node primordials/realm/process/loaders booting from the real runtime;
- full hostile import, reentrancy, GC, memory-grow, and teardown suite.

Exit gate: every imported engine symbol has a contract/test; the named
bootstrap/realm/process/CJS/ESM allowlist whose native dependency closure is
assigned to R1 loads without a host binding shim; later-subsystem modules are
explicit not-yet-implemented results and cannot pass through a stub; malformed
imports follow their frozen status/trap/termination classification; no
capability or backing-store reservation leaks after repeated create/destroy.

### R2 — shared POSIX substrate, libuv, fs, and addons

Deliver:

- reusable Rust WASI/POSIX host provider;
- sysroot/libc and kernel gaps required by libuv, threads, polling, fs, watches,
  and dynamic loading;
- actual libuv event loop driven only by generic readiness;
- fs sync/async/streams/watch and CJS/ESM loading through the WASM runtime;
- expansion of the R0-gated addon loader to the required addon corpus, or the
  approved first-cutover unsupported decision;
- cross-runtime fd/errno/readiness/fs fixtures;
- generated filesystem/mount/path/fd policy allow/deny matrix, including
  symlink/rename races and cross-VM probes.

Exit gate: fs/loaders/streams/async-context required identities pass; if R0
enabled addons, the addon corpus passes lifecycle tests, otherwise the approved
unsupported decision, `ERR_DLOPEN_FAILED` tests, compatibility manifest, and
docs are green; no high-level fs or host-emulated libuv service is exercised;
standalone WASM and Node see the same kernel objects and errno.

### R3 — sockets, DNS, HTTP, and undici

Deliver:

- libuv TCP/UDP/Unix sockets over generic kernel syscalls;
- c-ares and DNS;
- llhttp HTTP/1, nghttp2 HTTP/2, Node net/http/http2, undici, and fetch;
- backpressure, cancellation, half-close, timeout, keep-alive, and server
  lifecycle parity;
- generated socket/DNS/Unix-socket policy allow/deny matrix;
- deletion of replaced network/HTTP host services and polyfills.

Exit gate: local and external e2e tests pass through kernel sockets; protocol
state lives in WASM; slow-reader/writer and small-chunk benchmarks meet the
performance gate; socket exhaustion is typed and isolated.

### R4 — Node OpenSSL, TLS, crypto, and compression

Deliver:

- Node ncrypto/tls linked to the exact pinned OpenSSL 3.5.5 archives;
- platform CA policy and kernel TCP record path;
- Node crypto/WebCrypto, TLS client/server, zlib, Brotli, and zstd;
- black-box Node↔registry TLS tests without changing or depending on the
  registry stack's mbedTLS source/build;
- CPU/memory termination for KDF, crypto, and compression work;
- deletion of RustCrypto/emulated OpenSSL/high-level TLS/crypto/zlib paths.

Exit gate: `process.versions.openssl` reflects the live build; Node↔registry
TLS works in both directions; certificate/SNI/ALPN/session tests pass; no host
TLS termination occurs; one busy crypto VM cannot delay another VM beyond the
numeric p99 bound frozen in R0; networking-owned paths have zero diff.

### R5 — processes, pipes, PTYs, signals, and lifecycle

Deliver:

- libuv process/spawn/wait, pipes, stdio, PTYs, signals, process groups, and
  `beforeExit`/exit behavior over the shared kernel contract;
- real guest `node` command and self-spawn behavior;
- bounded process/thread/fd cleanup under cancellation and forced teardown;
- cross-runtime process/signal fixtures;
- generated PID/signal/process/credential policy allow/deny matrix;
- deletion of replaced child-process and signal bridge state.

Exit gate: child-process/stdio/signal suites and real command e2es pass;
teardown leaves no process, fd, wait, or callback; PID/errno/exit ordering
matches captured Linux fixtures.

### R6 — aggregate limits, snapshots, and performance

Deliver:

- aggregate V8/WASM/host/kernel memory reservation and reporting;
- combined CPU enforcement and attributable async kernel accounting;
- pinned-byte compile/instantiate path plus any V8 compiled-module cache or
  quiescent memory template enabled by the section 8 gates;
- all threshold warnings, typed errors, inventories, and operator docs;
- full compatibility/hostile/concurrency suite;
- complete cold/warm, throughput, crypto/compression, and concurrent-VM
  benchmark report;
- frozen rollback SLO, immediately previous signed artifact digest, and
  preserve/invalidate policy for compiled caches, optional memory templates,
  and running sessions.

Exit gate: all denial-of-service tests isolate the offender; if a memory
template ships, snapshot restore is behaviorally equivalent to clean boot and
uses unique entropy/identity, otherwise the release manifest proves the path is
disabled; every limit is bounded, warns, and names its configuration; no
unapproved performance regression exceeds 10 percent.

### R7 — staged cutover, deletion, docs, and release readiness

Deliver:

- R7a staged candidate with the new runtime selected internally, every gate
  green, and a rehearsed rollback to the immediately previous signed release
  within the numeric operator SLO;
- R7b deletion of the selector and legacy implementation followed by a clean
  rebuild, full retest/restage, upgrade rehearsal, and previous-release
  rollback rehearsal;
- complete legacy/polyfill/host-binding/bridge deletion inventory at zero;
- removal of the flavor flag and obsolete limits;
- updated architecture, security, resource, API, and operator documentation;
- reconciled root/scoped CLAUDE guidance and implementation-site comments for
  every import-object construction, syscall declaration/dispatch, and policy
  authorization site, with zero stale architecture claims;
- fixed-version, package, publish, release-asset, and generated-mirror checks;
- final native/legacy/new compatibility set-diff and release sanity artifacts.

Exit gate: every required compatibility identity is green with zero accepted,
skipped, missing, deleted, or weakened required tests; no performance exception
remains; no forbidden bridge symbol or legacy implementation remains; the
machine gate runner and all explicit expensive gates pass; rollback evidence is
current; the release artifact contains the pinned module and manifests.
Completion means release-ready staging. Publishing or deployment still
requires explicit authorization.

## 12. Required gates per revision

Run gates proportionate to the change, but no milestone may omit:

- vendor/import/export manifest checks;
- changed Rust/TypeScript/C/C++ format, lint, type, and unit checks;
- targeted runtime and hostile tests;
- suite set-diff against the preceding ratchet;
- limits-inventory and forbidden-symbol scans;
- build-artifact and generated-file leakage audit;
- documentation/spec update for changed contracts;
- an evidence-only child `E` whose checked
  `docs-internal/evidence/node-runtime-wasm/RN.json` records and tests its
  immutable implementation parent `C`, including commit/change IDs,
  source-tree manifest SHA-256, clean-tree proof, PST timestamps, environment,
  exact argv, status, duration, test counts, and SHA-256 input/output hashes.
  The gate verifies `parent(E) = C`, `diff(C..E)` is limited to evidence paths,
  and release artifacts are built from `C`. CI uploads immutable logs for `E`;
  local `~/progress` artifacts alone are not release evidence.

The expensive R7 gate includes `cargo check --workspace`, `pnpm build`,
`pnpm check-types`, publish helpers, workflow parsing, full pinned Node
categories, cross-runtime TLS/POSIX e2es, hostile/concurrency tests, benchmarks,
website build when changed, and release staging.

## 13. Stop conditions and escalation

The implementation does not fall back to the superseded architecture when a
port is hard. A blocker is escalated with:

- the exact upstream source and failing contract;
- the sysroot, generic import, kernel, EdgeJS-port, and Node-patch options
  attempted;
- a minimal reproduction and artifact;
- security/performance implications;
- the smallest decision needed.

“WASI/WASIX does not have it” is not a blocker. Implement the POSIX behavior.
“EdgeJS stubs it” is not acceptance evidence. Replace the stub or explicitly
scope the unsupported public feature. A final high-level Node host service
requires a new architecture decision and cannot be introduced as a workaround.
