# Spec: Real Node.js stdlib in agentOS (delete the reimplementation)

Status: READY FOR IMPLEMENTATION v4 (user decisions resolved; M0 evidence gates) · Owner: runtime team · Date: 2026-07-09 (PST)
POC evidence: jj workspace `node-stdlib`, change `wyqxqylo` (`c5888247`);
artifacts in `~/progress/agent-os/2026-07-09-node-stdlib-wasm-poc/`.
Review: 4-lens review integrated (dispositions in Appendix A); v4 applies the
2026-07-09 user decisions (§0).

---

## 0. Decisions log

2026-07-09, decider: user.

1. **Toolchain/sysroot — in the monorepo, part of the build pipeline**
   (resolves old OQ 4; supersedes review disposition C2's open question).
   "Building the nodejs stdlib wasm components should be part of the build
   pipeline. It should use the same patched libc that's part of the same
   monorepo." → `toolchain/` lands on `main`; stdlib leaf-lib wasm is built
   by the same pipeline against the in-repo patched libc (§3.2, §9.4).
   sha256-pinning of all toolchain inputs stays a hard requirement; prebuilt
   sysroot artifacts survive only as a CI-cache implementation detail.
2. **Crypto + TLS — one real OpenSSL 3.5.x wasm build, shared in-guest**
   (resolves old OQ 2 and old OQ 8). Build the exact OpenSSL source bundled
   by the pinned Node 24 tag once against the in-repo sysroot, owned by
   `toolchain/c` rather than the Node crate; Node
   `crypto`/`tls` and the registry C tools link/use that same wasm
   `libcrypto`/`libssl` build and the same VM CA bundle. TLS records flow over
   kernel-owned TCP; there is no host TLS termination. The current registry
   networking stack's mbedTLS build is proven migration input, not a second
   final backend: mbedTLS 3.6 has no OpenSSL 3 compatibility surface capable
   of satisfying node's ncrypto/EVP/provider contract. BoringSSL is rejected
   for the same API/behavior divergence. §7.4 defines the shared-build and
   cross-runtime acceptance gates; the RustCrypto `_crypto*` bridge and host
   `rustls-native-certs` paths move to deletion scope.
3. **execPath/self-spawn — real guest `node` command** (resolves old OQ 7).
   Governing principle, elevated to north star (§1): the guest is
   *"targeting native linux compatibility and it can't tell it's a
   different executor."* Current functionality is preserved during
   migration.
4. **Browser runtime — disabled and out of scope** (final user decision,
   superseding the earlier defer-and-document answer). Keep the browser
   runtime source, but remove it from CI, release, and publish build/test
   matrices before native cutover work begins. This program does not migrate,
   delete, keep buildable, or produce a design doc for the browser runtime;
   it cannot block CI or release. Re-enabling it is a separate program.
5. **Node version — Node 24.15.0 LTS.** Pin tag `v24.15.0`, commit
   `848430679556aed0bd073f2bc263331ad84fa119`, in the vendor manifest. The
   v26-nightly POC is evidence for the seam, not the shipped binding
   inventory; M0 reconciles the inventory against this v24 tag.
6. **Workers — unsupported at cutover.** The worker binding is inert data;
   `new Worker` throws the typed unsupported error. Workers do not gate
   cutover.
7. **Migration parity — measured legacy parity, including performance.** M0
   captures the current legacy implementation with the same suite and bench
   harnesses. The real path may be incomplete during staged implementation,
   but the default flip and deletion gate require at least the legacy
   supported-test set and no unapproved >10% performance regression; new
   native-node passes ratchet upward. Guessed runnable percentages are not
   acceptance criteria.
8. **Sequencing — dependency-driven, not a user product choice.** Keep the
   fs → event loop → net/http → crypto/TLS/child-process order unless measured
   implementation evidence requires a change. HTTP depends on the net/event
   loop substrate; moving it earlier would only trade schedule against fs
   completion, not change the end state.

---

## 1. Goals and non-goals

**Goal.** Guest JavaScript in agentOS VMs runs against the **real Node.js
stdlib** — the unmodified `lib/**/*.js` sources from `nodejs/node`, loaded by
node's own `internal/bootstrap/realm.js` — with the `internalBinding()` layer
provided by agentOS. All hand-written Node-API reimplementations are deleted.
Observable behavior matches **native Node on Linux**; deviations are bugs in
whichever agentOS layer owns them (kernel, VFS, bridge, sysroot/libc, event
loop) and are fixed there, never papered over (per repo `CLAUDE.md`).

**North star (user, 2026-07-09):** the guest is *"targeting native linux
compatibility and it can't tell it's a different executor."* Behave like
native Linux; anything the guest could observe to distinguish agentOS from
a real Linux node process is a defect.

**Non-goals (this program):**
- `worker_threads` with real parallelism (§10.4; decided unsupported at
  cutover: inert binding, `Worker` constructor throws typed error — see §4
  STUB policy).
- Inspector/profiler protocol support (`inspector`, `profiler` stay inert
  stubs; `config.hasInspector` pinned `false`).
- Compiling node's C++ core or libuv to wasm. Ruled out by feasibility study:
  node `src/*.cc` is inseparable from direct V8 API use (wasm cannot call the
  engine), and a guest-side libuv would own fds/sockets inside the untrusted
  guest — wrong side of the agentOS security boundary. The edgejs N-API port
  proves the alternative exists at whole-project cost; we adopt its *seam*
  (real `lib/`, `internalBinding` boundary), not its C++ port.

**POC results grounding this spec (all green):**
- Stage 1: real `path` + `buffer` + full transitive internals under node's
  real `realm.js`, in the embedded V8 runtime (~60 checks).
- Stage 2: real `lib/fs.js` **sync** ops against the real kernel/ChunkedVFS
  through `_fs*` sync-RPC in a full sidecar VM (ENOENT/errno/syscall correct).
- Stage 2.5: upstream **simdutf v9.0.0 compiled unmodified to wasm32-wasip1
  against our patched sysroot**, instantiated by V8's own wasm engine inside
  the isolate, backing buffer codecs (~30× faster than the JS shims; zero
  sysroot patches needed).
- Stage 3: **async** fs — FSReqCallback/FSReqPromise adapter, `fs/promises` +
  FileHandle, 10-way concurrency, node-exact nextTick/microtask/completion
  ordering on the marquee probe, and **real `internal/streams`**
  (createReadStream → pipe → createWriteStream, byte-identical) — with **zero
  runtime changes**; the session event loop's pending-promise accounting
  already provides liveness.

## 2. Architecture

```
┌────────────────────────────── V8 isolate (guest) ──────────────────────────┐
│  user code                                                                 │
│  node lib/**/*.js  (vendored verbatim, loaded by node's realm.js)          │
│  ── internalBinding(name) + process object ── the seam ────────────────────│
│  binding shims (JS, small, mirror node's C++ binding contract)             │
│   ├─ pure-compute → wasm leaf libs (simdutf/ada/llhttp/zlib/OpenSSL…)      │
│   │                 our patched sysroot, instantiated in-isolate (V8 wasm) │
│   └─ I/O → existing bridge globals (bridge-contract.json) ──sync-RPC──┐    │
└───────────────────────────────────────────────────────────────────────┼────┘
                                                                        ▼
                                    sidecar (trusted): kernel, fd table, VFS,
                                    sockets, processes, CSPRNG, permissions
```

Principles:
0. **Indistinguishability.** The guest can't tell it's a different executor
   (§1 north star). Design tiebreaker: when two implementations are
   otherwise equal, pick the one whose observable behavior (timing classes,
   errno, version strings, /proc-adjacent surfaces) matches native Linux
   node.
1. **Semantics live in node's JS.** We never fork `lib/` behavior. If node's
   JS misbehaves on our runtime, the bug is below the seam.
2. **The seam is `internalBinding` plus the process object.** In real node
   the `process` object, `process.env` interceptor, argv/execPath/versions/
   config are created in C++ *below* `internalBinding`; we synthesize them
   host-side to the same contract (§4.5). Everything else crosses only at
   `internalBinding`.
3. **The kernel plays libuv's role.** fds, sockets, processes, watches, and
   readiness belong to the trusted sidecar. Binding shims are marshalling
   glue, not policy. The **sidecar/kernel is the security boundary**; builtin
   allowlisting is API tiering on top of it, not the boundary itself (§5).
4. **Real native code where compute matters.** Upstream leaf libraries
   compile against `toolchain/c/sysroot` and run in-isolate. Sysroot/libc
   gaps are fixed **in the sysroot** (repo CLAUDE.md; policy text must also
   land in main's CLAUDE.md — M0 prerequisite, see §9.5).
5. **Linux parity is testable.** Every shim that emits or parses a
   kernel-visible format cites its authority in a code comment (man page,
   kernel source path, RFC, or node `src/*.cc` contract) and is covered by a
   conformance test naming a captured real-Linux/real-node fixture
   (prior art: `tests/fixtures/*-conformance.json`).
6. **Bounded by default.** Every new per-entity collection this program
   introduces gets a limit wired through `VmLimits`
   (`crates/sidecar/src/limits.rs`) and `limits-inventory.json`, warning near
   threshold and failing with a typed error naming the limit (§6.6).

## 3. Vendoring, bootstrap, and snapshot

### 3.1 Node version (DECIDED — Decision 5)
- **Pin Node 24.15.0 LTS:** tag `v24.15.0`, peeled commit
  `848430679556aed0bd073f2bc263331ad84fa119`. The POC ran a v26 nightly,
  `fbf82766d62`; it validates the architecture, not the shipped ABI. Node 24
  is the user-selected stable LTS line. This tag bundles OpenSSL 3.5.5; the
  vendor manifest records both source identities.
- The internalBinding ABI is unstable across versions. Accepted by the user
  (we control deployment). Requirements: pinned tag + sha in the vendor
  manifest, surfaced in docs; upgrades are deliberate PRs gated by the full
  conformance suite (§10).
- **M0 acceptance item:** re-run the `internalBinding(...)` grep diff between
  the v26 POC nightly and the pinned v24 tag and reconcile the §4 table (the nightly
  inventory already drifts — `permission` was missed on first pass).

### 3.2 Vendoring mechanics — how we get node's source

**Answering the "how do we get the nodejs source and patch it" question
directly:** we don't build node and we don't patch its JS. The POC ran
`path`, `buffer`, and `fs` (sync + async + streams) with **zero patches to
node's `lib/**/*.js`** — the stdlib is designed for exactly this injection
(every builtin receives `internalBinding`/`primordials` as parameters).
`scripts/vendor-node.mjs` copies `lib/**` **verbatim** from a git checkout
of the pinned release tag into the repo; that's the whole acquisition
story. If a vendored-source patch ever becomes unavoidable, it goes through
`crates/node-stdlib/vendor/patches/` (currently empty by design): numbered
`git format-patch` files applied by the vendor script at re-vendor time,
each with a header stating why the fix can't live below the seam — an
empty dir is a healthy signal, a growing one is a smell (per the repo's
patch-the-layer-that-owns-it policy).

- New crate **`crates/node-stdlib`** owns everything above the bridge:
  - `vendor/lib/` — node `lib/**/*.js`, `LICENSE`, plus the js2c JS deps set
    (`deps/undici`, `deps/acorn`, `deps/cjs-module-lexer`).
  - `vendor/native/` — **leaf-lib C/C++ sources vendored from the same
    pinned node tag** where node vendors them: `deps/llhttp` (the *generated
    C*, not upstream TS), `deps/ada` (amalgamation), `deps/nbytes`; simdutf
    from the dependency version selected by the pinned Node tag (or its own
    pinned upstream release if that tag does not vendor it); zlib/brotli/zstd
    from pinned upstream releases. Version skew between leaf libs and the JS
    that drives them is eliminated by construction. **OpenSSL is deliberately
    not owned here** because it is shared with registry software; its source
    identity and build live under `toolchain/c` (§9.4), and this crate records
    that shared manifest hash.
  - `bindings/` (shim JS, one file per binding), `src/` (Rust: source map,
    snapshot creator, HOST binding functions).
- `scripts/vendor-node.mjs` copies from a node checkout at the pinned tag,
  records `{tag, sha, files, sha256}` in `vendor/manifest.json`, fails on any
  local diff; CI re-runs it in check mode. Vendored files are never edited.
- **Leaf-lib wasm is built by the repo's own pipeline against the in-repo
  patched libc (Decision 1).** Build rules live in `toolchain/c/Makefile`
  (same idioms as zlib/mbedTLS/DuckDB); inputs are `vendor/native/` sources
  + the in-repo sysroot; outputs land as crate assets staged into `OUT_DIR`
  (precedents: pyodide asset staging in `publish.yaml:373-375`, CA bundle
  via build.rs) with a **typed hard error** when absent — no silent
  fallback, no committed binaries. Local dev: `just` recipe drives the same
  Makefile targets. CI/release build the blobs via the cached-sysroot job
  (§9.4); docker sidecar builds and darwin/linux release legs consume the
  same job's outputs, so bytes are identical everywhere (required if ever
  snapshot-adjacent).
- **Ownership boundary:** `crates/node-stdlib/wasm/crypto/` owns only the
  Node-facing C/C++ adapter ABI and JS shim. It must not contain a private
  OpenSSL source copy, patch set, download, or prebuilt archive.
- Builtin IDs mirror node's js2c mapping (`lib/foo.js` → `foo`,
  `deps/undici/undici.js` → `internal/deps/undici/undici`) so `realm.js`
  works unchanged.

### 3.3 Bootstrap + snapshot
Ground truth (verified): snapshots are created **lazily at first Execute in a
helper subprocess** and cached process-wide by `(bridge_code, userland)` key
(`crates/v8-runtime/src/snapshot.rs:307`, `session.rs:1434`); today creation
failure silently degrades to in-context eval (`session.rs:1462-1471`); each
Execute restores a fresh context (`session.rs:1526`).

Spec:
- Snapshot a context with primordials + `realm.js` + the eager core set
  (`buffer`, `util`, `errors`, `events`, `stream`, `fs`, `path`, `timers`,
  process setup from `internal/bootstrap/node.js`) — the set node itself
  snapshots (node `tools/snapshot`, `internal/v8/startup_snapshot`; the
  `mksnapshot` binding exists for this).
- **SnapshotCache key gains stdlib flavor + vendor-manifest hash** (flag A/B
  means two cache entries; coldstart budget accounts for double warmup while
  the flag exists).
- **V8 snapshots cannot serialize WebAssembly objects.** Leaf-lib wasm is
  therefore instantiated **post-restore** on first use (lazy), never inside
  the snapshotted context. The eager set's JS is snapshotted; its wasm-backed
  fast paths attach lazily (§8.4 budgets this).
- **HOST bindings in the snapshot require external-reference registration**
  (every FunctionTemplate/accessor registered with the SnapshotCreator).
  Consequence: moving a binding between JS and HOST after M0 is a
  snapshot-format change — plan the split in M0, not opportunistically.
- **The silent degrade path is removed** for the real stdlib: snapshot
  creation failure falls back to a *defined* full `realm.js` in-context
  bootstrap (same semantics, slower, logged with reason). Execute fails with
  a typed error only if that fallback also fails; it never enters a silently
  different environment.
- Non-snapshotted builtins compile lazily from the in-snapshot source map;
  per-builtin V8 code-cache blobs are a follow-up if the coldstart bench
  (§11.2's lazy-compile metrics) shows need.

## 4. Binding plan (v26-POC inventory; exact v24 count reconciled at M0)

Strategies: **JS** (pure JS shim) · **WASM** (JS shim calling in-isolate wasm
leaf lib) · **BRIDGE** (kernel/sidecar via bridge globals) · **HOST** (Rust in
`crates/v8-runtime` via rusty_v8) · **STUB** (inert, see policy).

**STUB policy (revised after review):** a stubbed binding is **inert data,
never throw-on-access**. Bindings are destructured at module load time all
over `lib/` (e.g. `internalBinding('worker')` at `internal/worker.js:74-85`
and inside `test/common`), so a throwing stub bricks bootstrap and the entire
test plan. Pattern: plausible inert values (`isMainThread: true`,
`threadId: 0`, `ownsProcessState: true`, …) with only the *action* entry
points (`new Worker`, `inspector.open`, …) throwing
`ERR_AGENTOS_UNSUPPORTED(<feature>, <docs link>)`. Workers follow Decision 6.

**M0 gate: every binding below loads inert** — `require()` of every public
module must succeed at M0 with bindings in at-least-inert form, because
load-time destructuring couples them (`lib/net.js:68-99` pulls
`uv/cares_wrap/stream_wrap/tcp_wrap/pipe_wrap` at module scope;
`test/common/index.js` requires `net`, `worker_threads`, `tty` at load).
Milestones then make families *behave*, not *exist*.

| Binding | Strategy | Notes / Linux-parity authority |
|---|---|---|
| builtins, options, symbols, errors, constants | JS | constants harvested from real Linux node (`internal/test/binding`), vendored fixture JSON w/ source version; `errno(3)`, `signal(7)` |
| config | JS | pins: `hasIntl:false` (initially, see §4.6), `hasInspector:false` (`console/constructor.js:693`, `util/inspector.js:85`); property-readable always |
| util | JS + HOST + BRIDGE | HOST: `getOwnNonIndexProperties`, `getProxyDetails`, `getPromiseDetails`, `getConstructorName`, `privateSymbols` (`bootstrap/node.js:81-83`); bootstrap-critical: `setupProcessObject`/`setupGlobalProxy`/`setupBuffer` (see §4.5); BRIDGE: `guessHandleType` (kernel fd-type info; Linux-parity fixture — TTY/pipe/file wiring of stdio) |
| types | HOST | exact V8 type checks (`v8::Value::Is*`) |
| buffer | JS + WASM | codecs/validators → simdutf + base64 (nbytes); `node_buffer.cc` is the contract; POC-proven |
| string_decoder, encoding_binding | JS + WASM | encoding enum order semantic (node `src/node.h`) |
| icu | STUB (inert) | `hasIntl:false`; guest `Intl` is unaffected (V8's own ICU, `isolate.rs:18-19,106`); see §4.6 for what degrades |
| url | WASM | ada from the pinned node tag; WHATWG URL spec cited at parse sites |
| url_pattern | JS/HOST | **not** plain ada-wasm: node's URLPattern uses ada with a `regex_provider` backed by V8 RegExp (`ada.h:4393-4649`); a wasm build would substitute a non-ECMAScript regex engine = forbidden deviation. Either route regex compile/exec back to JS `RegExp` via wasm imports, or implement the provider host-side |
| cjs_lexer | WASM | Node 24 replaced `cjs-module-lexer` with the pinned `deps/merve` native lexer; compile that exact source as an in-isolate wasm leaf library |
| fs, fs_dir | BRIDGE | §7.1; fd-level 1:1; errno via uv map; `open(2)`/`stat(2)` |
| fs_event_wrap | BRIDGE (new kernel primitive) | §7.2, `inotify(7)` |
| uv | JS (fixture) | `uv_errmap` generated from real node on Linux, vendored fixture |
| blob | JS + BRIDGE | in-memory Blob JS; file-backed via fs bridge |
| os | BRIDGE | kernel values match the VM's Linux persona (`uname(2)`) |
| process_methods, credentials | BRIDGE | uid/gid/cwd/umask/kill from kernel |
| permission | JS (+ future BRIDGE) | **load-bearing at require time** (`lib/fs.js:150` → `internal/process/permission`; also `child_process.js:98`, `fs/promises.js:113`, `pre_execution.js:654,706`). Ship `isEnabled:()=>false` (node permission model off — agentOS enforcement is the kernel, below the seam). Explicit later decision: surfacing agentOS permissions through node's `process.permission` API is possible but NOT this program |
| timers | JS→session | node's real `internal/timers` via `setupTimers(processImmediate, processTimers)` (POC stage 3); production backs it with `_scheduleTimer`/kernel timers |
| task_queue | JS + HOST | **not pure JS**: needs `setPromiseRejectCallback` (only path to `unhandledRejection`), `runMicrotasks`, `enqueueMicrotask` — all isolate-level, all exposed by rusty_v8. Must reconcile node's explicit `runMicrotasks()` with `run_event_loop`'s implicit checkpoints (§6.3) |
| async_wrap, async_context_frame | HOST | **AsyncLocalStorage requires V8 continuation-preserved embedder data** (`async_context_frame.js:8-11`, `src/async_context_frame.cc:38`) → verify our V8 build has `v8_enable_continuation_preserved_embedder_data` (M0 task); fallback path needs `setPromiseHooks` (`v8::Context::SetPromiseHooks`) — also HOST. `bootstrap/node.js:227` calls `async_wrap.setupHooks` unconditionally. Every callback-dispatching shim follows the MakeCallback discipline (§6.4) |
| stream_wrap, tcp_wrap, pipe_wrap, js_stream, stream_pipe | BRIDGE | §7.3 — StreamBase contract incl. sync/async duality |
| udp_wrap, cares_wrap | BRIDGE | `_dgram*`, `_networkDns*`; `getaddrinfo(3)`; c-ares behavior as contract |
| tls_wrap | WASM + BRIDGE | **Decision 2:** node's TLS contract over the shared in-guest OpenSSL wasm build; encrypted records use kernel-owned TCP via the socket bridge. RFC 8446 + node `crypto_tls.*` are the implementation authorities; §7.4 |
| http_parser | WASM | llhttp **generated C from the pinned node tag** (`deps/llhttp/src`); node `http_parser` binding contract |
| http2 | WASM + BRIDGE (M4) | compile the pinned node tag's nghttp2 to wasm and keep socket I/O in the kernel bridge; a host implementation is allowed only after a measured, documented nghttp2-wasm blocker |
| zlib | WASM | node needs BrotliEncoder + zstd compress (`node_zlib.cc:155,256`; `lib/zlib.js:89-92`) — **new build rules required**: toolchain currently builds brotli *decoder*-only and zstd *decompress*-only (built for curl). Add `c/enc` + `lib/compress` objects (zstd keeps `ZSTD_MULTITHREAD` undefined). Not "already built" |
| crypto | WASM | **Decision 2:** the pinned Node 24 tag's real OpenSSL 3.5.x, built once with our sysroot and shared with registry C tools. Binding surface is inventoried from the pinned v24 tag (the v26 POC saw ~174 properties) and is the checklist. M4 gate: crypto suite categories green with a real denominator (`versions.openssl` is real). §7.4 |
| sqlite | BRIDGE | existing `_sqlite*` globals |
| spawn_sync, process_wrap, signal_wrap | BRIDGE | `_childProcessSpawn*`, `waitpid(2)`, `signal(7)`; self-spawn via the real guest `node` command (Decision 3, §10.5) |
| tty_wrap | BRIDGE | `_kernelIsattyRaw`, pty; `tty(4)`, `termios(3)` |
| contextify, module_wrap, modules | HOST | real V8 compilation/contexts; §5 (CJS pulled forward to M1) |
| messaging | JS + HOST | far beyond MessageChannel: **DOMException lives here** (`util.js:711,716`), `js_transferable.setup()` runs at bootstrap (`node.js:149`), used by abort_controller, webstreams, crypto/random. HOST: structured clone via `serdes` ValueSerializer. Enumerate: DOMException, port drain as its own macrotask class, `receiveMessageOnPort`, transfer constants, `structuredClone` global |
| performance | JS + HOST | eventLoopUtilization from session loop stats; monotonic host clock |
| v8, heap_utils, serdes, internal_only_v8 | HOST | heap stats, Serializer/Deserializer via rusty_v8 |
| wasi, wasm_web_api | JS/HOST | expose node's `WASI` class over our existing in-isolate runner where sensible |
| diagnostics_channel, trace_events | JS | pure JS / validating no-op sink |
| locks, webstorage | JS / inert | Web Locks in-JS per spec. Web storage preserves current legacy behavior at cutover; any native-Node expansion beyond that baseline is post-cutover and cannot be advertised until its differential tests pass |
| block_list | JS | `node_sockaddr` semantics |
| worker | JS (inert) + STUB actions | **Decision 6:** inert data (`isMainThread:true`, `threadId:0`, `ownsProcessState:true`, …); only `new Worker` throws typed. Required for bootstrap (`is_main_thread.js:311`, `pre_execution.js:141,778`) and for `test/common` to load |
| inspector, profiler, report, watchdog, sea, quic | STUB (inert per policy) | quic revisit post-M4 |
| mksnapshot | JS | `startup_snapshot` runtime path (POC-proven); real mode used by our snapshot creator |

Bridge-global note: `crates/bridge/bridge-contract.json` defines **178**
`_*` globals with full `_fs*`/`_fs*Async` coverage — the fs/net substrate
exists; changes are shape adaptation plus the specific kernel additions in §7.

### 4.5 Process object synthesis (host-side, below the seam)
In real node the process object is built in C++ before any `lib/` runs. We
own the equivalent:
- `setupProcessObject`-equivalent creates `process` with correct prototype
  chain; `util` binding's `setupProcessObject/setupGlobalProxy/setupBuffer`
  entries participate in bootstrap exactly as `bootstrap/node.js:85-88`
  expects.
- **`process.env`**: named-property-interceptor semantics (coercion to
  string, `delete` behavior, enumeration order, no prototype pollution) —
  implemented as a rusty_v8 named-property-handler object (preferred) or a
  sealed Proxy with a **Linux-parity fixture** capturing real node behavior
  (set/get/delete/enumerate/spread/`in`).
- `process.argv` (guest argv from session), `process.execPath` (§10.5 —
  names the real guest `node` command, Decision 3), `process.versions`
  (node version from vendor manifest; `versions.openssl` is the **real
  version of the compiled OpenSSL** — Decision 2), `process.features`,
  `process.config` (a config.gypi-shaped
  frozen object — read by `lib/` and `test/common:37`; synthesized to match
  our build reality).
- `process.platform = 'linux'`, `arch` from VM persona.

### 4.6 What `hasIntl:false` actually degrades
Guest `Intl`, `String.prototype.localeCompare`, etc. **already work** — V8
carries its own ICU (`icudtl.dat`). `hasIntl:false` only disables node-side
ICU surfaces: `buffer.transcode`, the ICU-backed long tail of TextDecoder
encodings (whatwg subset still available via `encoding_binding`+simdutf),
and ICU IDNA (URL IDNA comes from ada's own implementation). Decision: ship
`hasIntl:false`; revisit only if suite/differential results show real
packages breaking on these three surfaces (disposition of old OQ 5).

## 5. Module loader

**Position: adopt node's real loader JS; keep our Rust resolver as the
policy oracle; sequence CJS early.**

- Node's `internal/modules/cjs/loader.js` + ESM loader run verbatim, reaching
  disk only through the `fs` binding and compilation through
  `module_wrap`/`contextify` (HOST). pnpm layouts resolve naturally because
  resolution does real readlink/stat against the real VFS.
- **Sequencing (review-corrected):** running *any* node-suite test file—or
  any user entrypoint—requires loading user code. Real-loader adoption can't
  wait for M5:
  - **M0:** interim documented exception — the existing legacy loader loads
    user code *into* the real-stdlib context (`AGENTOS_JS_STDLIB=real`),
    limited to entrypoint + CJS require. This is the only sanctioned
    real/legacy mixing, it is temporary, and it is listed as debt with an M1
    expiry.
  - **M1:** HOST `module_wrap` minimal surface +
    `compileFunctionForCJSLoader` land; node's real CJS loader takes over
    user code. Legacy-loader exception removed.
  - **M5:** ESM loader, `vm` module completeness, loader hooks.
- **Builtin availability policy (review-corrected):** the earlier "denied
  builtin's source is simply absent" is unsound with one shared snapshot and
  per-execution allowlists (`javascript.rs:3192` installs the gate per
  execution; the snapshot cache is process-wide, `session.rs:1434`) — and
  node's own bootstrap consumes `internalBinding('fs')`/`process_methods`,
  so binding-level denial breaks bootstrap. Spec:
  - There is an **undeniable bootstrap set** (what `internal/bootstrap/*`
    itself needs).
  - Per-execution denial is a **post-restore gate**: after context restore,
    a host step deny-wraps the realm's builtin map + `getInternalBinding`
    for user-deniable modules per this execution's allowlist. Denied modules
    throw node-authentic errors.
  - **Framing:** the allowlist is **API tiering**, not the security
    boundary. Bridge `_*` globals remain guest-reachable; the kernel/sidecar
    enforces actual policy. (This framing change is normative for docs.)
- Client-visible consequence (§12.4): denial error shape may change;
  `allowed_node_builtins` is public client config (Rust
  `crates/client/src/config.rs:34-35,87`, TS options schema, docs) —
  lockstep same-change updates required.
- Deleted at the end state: `bridge-src/builtins/module-loader.ts`,
  `builtin-modules.ts`, CJS export-name extraction in
  `crates/v8-runtime/src/execution.rs`, loader-hook templates in
  `node_import_cache.rs` (§12.1 split). Interop tests
  (`crates/execution/tests/cjs_esm_interop.rs`, `module_resolution.rs`) are
  the parity gate and must pass unchanged.

## 6. Event loop and timers

**Contract:** node's loop semantics per libuv's phase model (timers →
pending → poll → check(setImmediate) → close), nextTick + microtasks drained
between macrotasks, liveness = refed handles/requests > 0 (libuv design
docs; `deps/uv/src/unix/core.c:uv_run`; node `src/env.cc` DrainTasks).

**Verified current state:** `run_event_loop()`
(`crates/v8-runtime/src/session.rs:2202`) + pending-promise accounting
already deliver correct fs/streams ordering (POC stage 3, zero runtime
changes). Known deviations: single queue (no phases); deterministic-FIFO
`setImmediate`-vs-completion; session timers not libuv clock.

Spec:
1. **Phase-tagged macrotask queue** (timers, io-completions, check, close,
   plus a `messaging` port-drain class) in the session loop; nextTick +
   microtasks drained between items. Evolution of `run_event_loop`, not a
   rewrite.
2. **HandleRegistry** (JS-side, one per context): binding shims register
   refable entities (sockets, servers, watchers, in-flight reqs, timers);
   backs `internal/timers` and generalized loop-exit accounting
   (`_getActiveHandleInfo` superseding `_getPendingTimerCount` +
   `_waitForActiveHandles`). ref/unref matches node (`timer.unref()`,
   `server.unref()`).
3. **Microtask policy:** node's `task_queue` calls explicit
   `runMicrotasks()`; our loop also runs implicit checkpoints. Rule: the
   isolate runs with `MicrotasksPolicy::kExplicit` under the real stdlib and
   the loop performs the checkpoints exactly where node's DrainTasks does —
   otherwise double-draining reorders nextTick vs microtasks.
4. **MakeCallback discipline:** every shim-dispatched callback enters
   through one `runCallback(frame, cb, args)` helper that exchanges the
   async-context frame (CPED), emits before/after hooks, then drains
   nextTick + microtasks — matching node's MakeCallback. (Required by
   §4 async_wrap/HOST.)
5. **Loop-turn-cached clock:** node caches `uv_now` per loop iteration
   (`internal/timers.js:387` getLibuvNow); a fresh clock per call splits
   timer lists and changes firing order that suite tests encode. Our now()
   is cached per loop turn, invalidated at phase boundaries.
6. **Limits (bounded-by-default):** HandleRegistry size, pending
   io-completions, watch descriptors + queued events (§7.2), contextify
   contexts, wasm module cache entries — each in `VmLimits` +
   `limits-inventory.json` with typed errors.
7. Fidelity budget: during migration, every known phase deviation enters the
   parity ledger (§10.3) with a linked upstream test. At cutover, a deviation
   may remain only when it is an explicit program non-goal (such as Workers)
   or preserves the measured legacy baseline while a linked post-cutover
   issue tracks the native-parity gap. The ledger cannot silently redefine
   observable Linux behavior as conformant.

## 7. I/O surfaces

### 7.1 fs
- **1:1 fd ops.** Extend/verify fd-level bridge globals (open/read/write/
  close/fstat/ftruncate/fsync/fdatasync/futimes/fchmod/fchown, readv/writev,
  positional I/O) so `internalBinding('fs')` maps one-to-one; delete the
  POC's whole-file emulation. Authority per call: `open(2)`, `pread(2)`, …
- **Binary transport, no base64** (§8.1) — landed in M1 with an A/B
  measurement proving the win (§11.2).
- Kernel/VFS additions where absent: `statfs(2)`, real `access(2)`,
  nanosecond `utimes`, `mkdtemp`, `copyFile` (+`COPYFILE_EXCL`), streaming
  `opendir` (`fs_dir`), Linux-faithful `realpath`, and **stat truth**:
  dev/ino/nlink/blksize/blocks real from VFS (authority `inode(7)`; captured
  real-Linux fixture).
- Known kernel deviation fixed now (POC friction log): readdir on missing
  dir returns empty instead of ENOENT — kernel VFS fix + parity fixture.

### 7.2 fs.watch
- New kernel primitive: VFS change events per watch descriptor with
  `inotify(7)` semantics (coalescing, `IN_MOVED_FROM/TO` cookie pairing
  where feasible, overflow signaling), bounded queues per §6.6. Bridge
  subscription → `fs_event_wrap`. `watchFile()` (stat polling) ships first.

### 7.3 net / http
- `stream_wrap`/`tcp_wrap`/`pipe_wrap` implement node's StreamBase contract
  over the socket bridge. **Review-corrected contract details (the real M3
  bar):**
  - **Sync/async write duality**: `writeBuffer` may complete synchronously,
    returning negative errno, with completion mode reported via the
    `streamBaseState` aliased array (`kLastWriteWasAsync`,
    `stream_base_commons.js:9-17,53`); `req.async=false` skips afterWrite
    ticks. Our shim implements the small-write sync fast path (bridge sync
    call under threshold) so observable ordering matches node.
  - **Reads** land in pooled ArrayBuffers with `kArrayBufferOffset`/
    `kReadBytesOrError` side-channels; `streamBaseState` is owned by the
    net shim layer and shared with `lib/internal/stream_base_commons.js`
    unchanged.
  - **`onconnection(err, clientHandle)`** must deliver a hydrated accepted
    handle (kernel accept-queue event carries the new socket id; shim wraps
    it in a ready TCPWrap).
  - Kernel additions: readiness push events (today's poll is pull-based for
    wasm commands) and accept-queue events. Authority: node
    `src/stream_wrap.cc` + `tcp(7)`, `unix(7)`; errno-path fixtures from
    real node (ECONNREFUSED/ECONNRESET/EPIPE).
- **http**: real `lib/_http_*.js` + `http_parser` binding backed by llhttp
  (generated C from the pinned tag) compiled to wasm. Deletes `http.ts`
  (4,558 lines) + undici-shims; `fetch` = vendored real undici over real
  net. http2 in M4.
- dgram/dns: `udp_wrap`/`cares_wrap` over `_dgram*`/`_networkDns*`;
  `dns.lookup` keeps `getaddrinfo` semantics distinct from resolver queries.
- child_process: `spawn_sync`/`process_wrap` over `_childProcessSpawn*` +
  kernel pipes; stdio inheritance and exit/signal codes per `waitpid(2)`;
  self-spawn story in §10.5.

### 7.4 crypto / TLS / zlib (DECIDED — Decision 2)
- **One real OpenSSL 3.5.x build:** use the exact OpenSSL source bundled by
  the pinned Node 24 tag through the shared `toolchain/c` manifest, compile it
  once with the in-repo sysroot, and stage
  one content-addressed set of wasm32 static archives (`libcrypto.a` and
  `libssl.a`). Node's
  `internalBinding('crypto')` and `tls_wrap` adapters use it in-isolate; curl,
  wget, git/libcurl, and full-crypto ssh use the same build in their in-guest
  wasm processes. All consumers use the VM's
  `/etc/ssl/certs/ca-certificates.crt` trust root.
- **Why this supersedes the WIP networking-handoff recommendation:** the
  current registry stack proves in-guest TLS, getentropy, CA-bundle, and
  socket plumbing with mbedTLS 3.6, but mbedTLS does not expose an OpenSSL 3
  compatibility API. Building such a façade would be a new crypto-abstraction
  project and still would not reproduce node's EVP/provider/error behavior.
  BoringSSL likewise diverges. Real OpenSSL is therefore the one shared
  backend; port gaps are fixed in the sysroot or OpenSSL build patches under
  the repo's one-layer-down policy, not with per-tool TLS adapters.
- **Build spike:** start in M0 and gate M1 on compiling `libcrypto` and
  `libssl`, loading providers without `dlopen`, seeding from `getentropy`, and
  completing an in-guest TLS handshake. If a concrete blocker remains after
  sysroot and OpenSSL patches are exhausted, record the failed surface and
  evidence as a refuted thesis and return for an architecture decision; do
  not silently introduce another backend.
- **Entropy:** OpenSSL RAND seeds via `getentropy(2)` → host import →
  kernel CSPRNG (precedent: the mbedTLS getentropy shim in `toolchain/c`).
  Seeding must never silently fall back to weak entropy: the getentropy
  import is mandatory and failure is a typed error (`getrandom(2)`
  semantics cited at the shim).
- **Consequences:** `process.versions.openssl` is **real** (old OQ 8
  deleted — `common.hasCrypto` suite gating works naturally; the M4 crypto
  acceptance has an honest denominator by construction). The crypto shim
  marshals through the same wasm import-object contract as other leaf libs
  (§9.4); streaming EVP/SSL contexts live in wasm memory keyed by handle,
  zeroized on free. The RustCrypto-backed `_crypto*` bridge globals + their
  sidecar handlers move to the **deletion inventory** (§12.1),
  protocol-lockstep removal at M5.
- **Size/coldstart:** the OpenSSL blob is multi-MB — §8.3 gains a
  crypto-blob budget; instantiation is lazy on first `require('crypto')`
  (post-restore, §3.3); measured by the §11.2 wasm lifecycle bench.
- webcrypto rides the same OpenSSL-wasm backend.
- **TLS is in-guest:** node's `tls_wrap`/`SecureContext` contract runs against
  real OpenSSL SSL objects in wasm; TLS records flow over kernel-owned TCP.
  The kernel retains socket and egress policy and sees ciphertext exactly as
  a Linux kernel does for native node. The host-owned
  `_netSocketUpgradeTlsRaw`/`rustls-native-certs` path is deleted at cutover.
  Client-chosen TLS credentials and options retain the same trust position as
  native Node.
- **Shared-backend acceptance:** build/link manifests prove every Node and C
  consumer links the same archive/source hashes (final wasm modules may each
  embed their linked copy); a cross-runtime e2e runs curl
  and Node `https.get`/`tls.connect` against the same trusted and self-signed
  servers, checks the same success/failure class, and verifies `--cacert` and
  `NODE_EXTRA_CA_CERTS`; no host trust store or per-tool TLS adapter remains.
- **zlib: wasm** with the §4 build-rule additions (brotli encoder, zstd
  compress). Node `node_zlib.cc` contract (streaming, flush modes,
  dictionaries). Deletes the bridge zlib payload (`v8-bridge-zlib.js`).

## 8. Copies and performance

### 8.1 Transport
- **Kill base64** on fs/net paths (M1, A/B-measured).
- **Sync zero-copy**: host writes directly into the guest's pre-allocated
  buffer via its backing store (`get_backing_store().data()` — the store is
  *obtained from* the guest buffer, not created host-side) strictly within
  the sync-RPC parked window. M1 includes a GC/detach stress test for the
  parked-window assumption.
- **Async is NOT zero-copy into live buffers**: `fs.read(fd, buffer, cb)`
  and stream reads target caller buffers while guest JS runs; host writes
  outside the parked window are a data race into the isolate heap. Rule:
  async completions carry host-owned bytes and are **copied into the guest
  buffer on the V8 thread at completion dispatch**. Host-side buffer
  pre-allocation is bounded (§6.6).
- Validation: detached/shared/resizable ArrayBuffers and length bounds
  rejected with typed errors at the shim.
- JS↔wasm copies for codecs stay (measured noise; stage 2.5).

### 8.2 Chattiness
- Order: (1) measure (binding-RTT microbench, §11.2); (2) batch
  resolver-shaped patterns (`_batchResolveModules` precedent); (3) node's
  default `highWaterMark`s stay node-default; (4) in-isolate caching only
  where node itself caches (e.g. realpath cache).
- **M5-entry bridge-call census**: stat/open calls per `npm zod import` and
  `require_100_small` (existing modules-bench rows) before/after real
  loader — the loader must not multiply bridge calls.

### 8.3 Budgets — measured-floor × headroom (M0 amendment, 2026-07-09 PST)
The M0 debug-sidecar early-warning capture is
`packages/runtime-benchmarks/results/node-stdlib-m0-baseline.json`. Release
milestones rerun the identical protocol on pinned hardware; these formulas,
not the debug timings, are normative. A floor that already exceeds its budget
is recorded in `node-stdlib-regression-ledger.json` rather than hidden by
moving the target.
- **Primary migration-parity gate (Decision 7):** at default flip and M5,
  every applicable p50 metric is no more than 10% slower than the same-day,
  same-machine legacy path. p99 and dispersion are published and any material
  tail regression requires a ledger entry. Native-node comparisons remain
  optimization targets; they do not excuse a regression from what ships now.
- Sync binding RTT: M0 legacy floor p50 0.04ms, p99 0.05ms (five measured
  samples after warmup); target p50 ≤ `max(10µs, 1.2 × floor)` = 0.048ms;
  p99 ≤ 5× p50. Metric: **p50/p99 of ≥1000 iterations, ≥5 runs, IQR
  dispersion reported**. The release floor run expands the M0 five-sample
  smoke to the full iteration/run count before this gate can fail a release.
- `readFileSync` 4KB / 1MB: M0 native-Node floors are 0.02ms / 0.28ms p50;
  M1 pre-transport budgets are `3 × floor` = 0.06ms / 0.84ms and post-
  backing-store budgets are `1.5 × floor` = 0.03ms / 0.42ms.
- Buffer codec ops over the 128KiB fixture: native Node floors are 0.34ms
  UTF-8 and 0.2664ms base64 round trip (nine samples; p99 0.8231ms / 0.6515ms;
  IQR 0.0428ms / 0.0208ms). Budgets are `2 × floor` = 0.68ms / 0.5328ms.
- Stream throughput (100MB fs pipe): ≥ 70% of native node at M3 exit.
- npm-import storm (`npm zod import`, `require_100_small`): M0 legacy floors
  are 174.49ms / 46.50ms p50; M5-entry budgets are `1.5 × floor` =
  261.74ms / 69.75ms.
- Session coldstart: M0 `fs`+`http` legacy floor is 518.92ms p50; budget is
  `1.1 × floor` = 570.81ms. **Snapshot blob size:** legacy 463,234 bytes,
  budget `2 × floor` = 926,468 bytes (real M0: 479,509). Snapshot build:
  legacy 12.798ms, budget `1.5 × floor` = 19.197ms (real M0: 13.343ms).
  All 71 per-builtin compile timings are recorded with no hard budget.
- **Crypto blob (Decision 2):** OpenSSL wasm ≤ 8MB stripped (measured at
  first build; renegotiated via ledger if the floor lands higher);
  first-`require('crypto')` lazy instantiation ≤ 50ms p50 warm (code-cache
  assisted); sessions that never touch crypto pay zero (lazy, §3.3).
- **Wasm lifecycle:** the 2,437,405-byte shared OpenSSL handshake module has
  cold compile p50 3.60ms, cold instantiate p50 0.59ms, and warm instantiate
  p50 0.54ms with V8's native-module cache enabled (five fresh-isolate samples).
  Sharing the compiled module across isolates reduces worker wall p50 from
  30.62ms to 21.56ms; the full cache-on/cache-off distributions and module hash
  are in `results/node-stdlib-m0-wasm-lifecycle.json`.
- `bench:baseline`/`bench:matrix`: no metric regresses >10% (nightly gate,
  §11.3) without a ledger entry.

### 8.4 Snapshot & wasm caching
- Builtins in snapshot (§3.3); wasm instantiation is post-restore and lazy.
- **M0/M1 microbench (named deliverable):** leaf-lib compile + instantiate,
  cold/warm, with/without V8 code cache and with/without cross-isolate
  module sharing — cross-isolate sharing is an assumption to validate, not
  a design input. Feeds the coldstart budget.
- Compiled-module cache bounded per §6.6.

## 9. libc / sysroot workstream (parallel track — NOT on the cutover path)

**Review-corrected dependency map:** none of simdutf/ada/llhttp/zlib/brotli/
zstd need eventfd, epoll, or threads — they are pure-compute reactors (POC:
3 stub WASI imports). The libc items below serve future registry software
and the "behave like Linux" north star. They form a **parallel workstream
(L1–L3) with its own milestone; M5 cutover does not block on them.**

### 9.1 L1 — eventfd
- Kernel: new fd-table object type (event counter, semaphore mode) owned by
  the sidecar fd table; host import exposed to guests; **pollable** (the
  libuv eventfd-in-epoll pattern is the acceptance test), so the kernel
  PollNotifier must treat it as a readiness source. libc: `eventfd(2)`
  semantics over the host import (header exists today, impl absent —
  verified).
- Conformance: eventfd-inside-epoll case in `toolchain/conformance` (or its
  post-§9.4 home).

### 9.2 L2 — epoll
- Approach: **libc-level emulation over host poll first** (level-triggered
  `epoll_create1/ctl/wait` over `poll_oneoff`/host_net poll — `epoll(7)`
  core semantics); a dedicated host import against PollNotifier only if
  emulation hits a measured wall. Edge-triggered later. Header currently
  denylisted — remove from denylist with the impl.

### 9.3 L3 — threads
- Stay emulated (documented wall; V8 wasm threads + SAB in-guest is a
  separate program). All program leaf libs compile with
  `-D_WASI_EMULATED_PTHREAD` (proven).

### 9.4 Toolchain in the monorepo + build integration (DECIDED — Decision 1; M0 prerequisite for M1 wasm)
- Ground truth: today nothing on `main` builds a C sysroot — `toolchain/`
  exists only on the reg-tests branch; the publish "registry WASM commands"
  job is cargo-only (`publish.yaml:53-78`); sysroot bootstrap = wasi-sdk
  download + wasi-libc clone/patch + LLVM-runtimes rebuild. **MEASURED
  (2026-07-09, 20-core linux, fresh dir, cold caches): 87s total** —
  wasi-sdk download+extract 15s, wasi-libc clone 2s, llvm-project tarball
  fetch 50s (network-bound), patch + libc build + libc++/libc++abi/libunwind
  build ~20s. Leaf-lib (simdutf) build against the fresh sysroot: 2s. The
  fresh libc.a verified to contain our host-import surface
  (`__host_net_socket`, `__host_proc_spawn`, …). Earlier "multi-hour"
  characterizations were wrong. Expect low single-digit minutes on 2–4 core
  CI runners (compute step is the only part that scales with cores; the
  ~65s of network fetches dominate and don't).
- **Decision (user): the toolchain lives in the monorepo and the stdlib
  wasm components are built by the same pipeline against the same in-repo
  patched libc.** Landing plan (M0): move `toolchain/` (`c/`,
  `std-patches/`, `scripts/`, `conformance/`) onto `main` as-is; the
  `crates/node-stdlib/wasm/` build rules join `toolchain/c/Makefile`; the
  release workflow gains a leaf-lib build job ordered before the sidecar
  builds (staged like pyodide assets).
- **Shared OpenSSL ownership and layout (normative):**
  - `toolchain/c/openssl/manifest.json` pins Node `v24.15.0`, its peeled
    commit, OpenSSL 3.5.5, the Node source archive sha256, the
    `deps/openssl/openssl` tree hash, configure flags, and expected outputs.
  - `toolchain/c/scripts/build-openssl-upstream.sh` is the only acquisition
    and build entry point. It extracts the exact Node-bundled source into the
    ignored toolchain cache and builds static, builtin providers without
    network access after acquisition.
  - `toolchain/c/patches/openssl/` contains numbered source patches only when
    a sysroot fix cannot own the gap; each patch states why.
  - `toolchain/c/build/openssl/{include,lib/libcrypto.a,lib/libssl.a,
    manifest.json}` is ignored build output. The output manifest includes
    source, patch-set, sysroot, compiler, flags, and archive hashes.
  - `crates/node-stdlib/wasm/crypto/` contains the Node binding adapter and
    links the shared archives. curl/libcurl, wget, git, and OpenSSH consume
    the same output manifest and archives. No consumer downloads or builds
    OpenSSL privately, and no `build.rs` performs a network fetch.
- **CI/dev delivery** (measurement above collapses the problem — the cold
  build is ~90s local / minutes on CI, so caching is an optimization, not
  load-bearing infrastructure; in-repo toolchain source is the single
  source of truth and a cold run reproduces the identical sysroot from
  pins alone):
  1. **Recommended: GitHub Actions cache with build fallback.** Content-hash
     key (wasi-sdk pin, llvm-project pin, `std-patches/**`,
     `wasi-libc-overrides/**`, build scripts); hit restores in seconds,
     miss rebuilds in-job in minutes — cheap enough for the PR path. The
     network fetches (wasi-sdk + llvm tarballs, ~65s of the 87s) should be
     mirrored to R2 with sha256 verification to remove third-party
     availability from the critical path.
  2. Optional layer: a sha256-addressed prebuilt sysroot artifact (R2) for
     local dev that wants `cargo build` to work without make — nice-to-have
     given the from-source path costs ~90s; `just build-sysroot` is the
     default local answer.
  Local dev is never blocked on a build it didn't ask for; any artifact
  fetch failure is a typed hard error naming the expected hash — not a
  silent fallback.
- **Pin everything (hard requirement, unchanged):** wasi-sdk tarball,
  llvm-project checkout, zlib/mbedtls/brotli/zstd tarballs, curl fork
  (currently tracks `main`!), os-test/libc-test — all get sha256/commit
  pins (today only the leaf-lib fetches are pinned — verified). Binaryen:
  vendor a pinned+sha256 `wasm-opt` or drop the step; identical in CI and
  local (wasi-sdk 25 ships none; today's Makefile silently skips).
- **Leaf-lib import-object contract:** minimize WASI imports; `fd_write`
  routes to a host-visible log (never a silent `→0` stub — that swallows
  malloc-abort diagnostics, a CLAUDE.md violation); every other import traps
  with a typed error naming the module.

### 9.5 Policy text
- The "fix one layer down" sysroot policy this spec cites lives only in the
  reg-tests CLAUDE.md today. Landing that normative text in **main's**
  CLAUDE.md is an M0 prerequisite so this spec's references resolve on main.

## 10. Testing

### 10.1 Real node test suite (the conformance gate)
- Harness `test-harness/node-suite/` (edgejs `test/nodejs_test_harness` as
  model): runs the vendored `test/parallel` + `test/sequential` against
  agentOS sessions; maps pass/fail/timeout.
- **Suite realism (review-quantified, of 3,893 test/parallel files):**
  ~16.5% (643) touch `child_process`/`process.execPath` (§10.5), ~8% (313)
  need Workers (skip(worker)), ~20.7% (807) gate on `common.hasCrypto`
  reading `process.versions.openssl` (mooted by Decision 2: the compiled
  OpenSSL makes the version string real — otherwise the whole crypto
  category self-skips and an M4 "crypto green" would be vacuous), ~15%
  (579) carry `// Flags:` directives (330 need `--expose-internals`).
  Harness requirements: parse `// Flags:` and emulate the relevant ones
  (`--expose-internals` → provide `internal/test/binding` per node's own
  mechanism; unsupported flags → skip(flag:<name>) with reason).
  **The denominator is measured, not guessed (Decision 7):** M0 runs the same
  harness against legacy and real paths and publishes both runnable/pass sets.
  The real path's M0 gap is implementation work, not an accepted percentage.
  Each milestone must preserve every legacy-passing test in the categories it
  replaces; by default flip/M5, the real path contains the complete
  legacy-passing set except explicit program non-goals, with all additional
  native-node passes ratcheted. Workers (~8%) and genuinely unsupported flags
  remain reasoned exclusions. The ledger states exact counts and set diffs at
  every milestone.
- **Expected-state ledger** (checked-in JSON per category): `pass`,
  `fail-accepted(reason, issue)`, `skip(reason)`. CI fails on regression
  AND on unexpected pass (forces ledger updates; progress visible in
  diffs). Ratchet: pass-count only goes up.
- **CI tiers (review-corrected to repo policy):** default CI runs a
  handful of sanity suite tests only; the ~200-test smoke slice runs
  nightly (`ci-nightly.yml`); full suite nightly + pre-release.
  Limit-saturating node tests are skip-by-default with reasons (repo test
  policy).

### 10.2 Linux-parity fixtures
- For kernel-owned behavior (errno, stat fields, signal codes, socket
  errors, env-interceptor semantics, guessHandleType): fixtures captured
  from **real Linux + real node** (regeneration script in-repo; fixtures
  name kernel/node versions), per `tests/fixtures/*-conformance.json`
  precedent. These gate the kernel/VFS/bridge.

### 10.3 Differential tests + parity ledger
- Differential runner: same script under native Linux node and agentOS,
  diff observable output. Used heavily in migration; subsets graduate to
  fixtures. Event-loop ordering deviations live in the parity ledger with
  linked upstream tests (§6.7).

### 10.4 worker_threads stance
- **Decision 6:** cutover ships without workers; binding is inert data (§4),
  `new Worker` throws typed. Suite marks `skip(worker)`. A future host-side
  multi-isolate + MessagePort design is a separate program and does not gate
  this cutover.

### 10.5 execPath and self-spawn (DECIDED — Decision 3)
- 643 suite tests (and real-world tools: npm lifecycle scripts, node-gyp
  wrappers, CLI re-exec patterns) spawn `process.execPath`. **Decided:**
  ship a real guest `node` command under `/opt/agentos/bin/` whose
  kernel-side spawn handler starts a fresh agentOS JS session — a real
  kernel process-table entry with a real pid, argv, env inheritance, stdio
  pipes, exit/signal codes through `waitpid(2)` semantics, like any spawned
  guest process. `process.execPath` names it; `child_process.fork` works
  (IPC channel over a kernel pipe with node's serialization); the guest
  cannot tell it re-executed a different executor (§1 north star).
- Lands with child_process in M4; current spawn functionality is preserved
  throughout migration (the command is additive to the existing
  `_childProcessSpawn*` path, not a rewrite of it).

## 11. Benchmarks: before/after + regression handling

### 11.1 Ground truth: there are TWO harnesses (review-corrected)
- `pnpm bench` at repo root → `scripts/benchmarks/run-benchmarks.sh` →
  lanes (coldstart-sleep, memory-sleep, memory-pi-session, session) against
  **agentos-sidecar**.
- `packages/runtime-benchmarks` (own `run-benchmarks.sh`, `bench:baseline`,
  `bench:matrix`, `bench:coldstart`, `bench:memory`, `bench:gate`) against
  **agentos-native-sidecar**, baselines at
  `results/baseline-{ci,local}.json` via `baselinePathForEnvironment()` (no
  `baselines/` dir).
- **Authoritative harness for this program: `packages/runtime-benchmarks`**
  (it targets the runtime this program changes and has the gate plumbing);
  the root lanes are tracked secondary. Consolidation is optional, not a
  dependency. M0 records exact per-harness commands + environment-keyed
  baseline paths in the bench README.

### 11.2 M0 measurement deliverables (before any cutover work)
1. **Floor measurements**: run `sync-bridge-floor.bench.ts` (exists) and a
   new native-node codec bench; record distributions (≥5 runs, p50/p99,
   IQR); **rewrite §8.3 budgets as floor × headroom** in a spec amendment.
2. **Dual-target runner** (new): the micro/macro benches (binding RTT, fs
   micro incl. readFileSync 4KB/1MB/stat/readdir-1k, buffer codecs, stream
   throughput, import storm, coldstart, RSS) runnable against (a) agentOS
   legacy, (b) agentOS real-stdlib, (c) **native Linux node**. Fairness
   protocol: native uses tmpfs (removes ext4-vs-ChunkedVFS storage
   conflation), same machine, same file sizes, declared cache state, fixed
   warmup, N and dispersion documented.
3. **A/B tooling**: flag-matrix runner (legacy vs real in one invocation)
   + PR delta-comment job (new CI job posting the delta table on
   runtime-touching PRs).
4. **Snapshot metrics**: blob size, build time, per-builtin lazy-compile
   timings; coldstart bench variant that actually `require`s fs/http (the
   existing coldstart lane is a sleep workload and would mask lazy-compile
   regressions).
5. **Wasm lifecycle microbench** (§8.4).
- M0 also freezes a full both-harness capture as an **early-warning
  reference only** — the *published* before/after for the program is a
  **same-machine, same-day legacy-vs-real A/B at each milestone and at M5**
  (a frozen M0 file vs an M5 run on different hardware is methodologically
  invalid).

### 11.3 Regression gates (reconciled with existing infra)
- **PR gate (cheap tier):** existing `bench:gate` semantics stay (threshold
  2.0× per quick-gate.ts — catastrophe catch on shared runners), extended
  with binding-RTT + fs-micro rows.
- **Nightly gate (authoritative):** full matrix on pinned hardware (or,
  where shared runners are unavoidable, gate on **agentOS/native-node
  ratios**, which cancel machine noise); metric = **p50 ratio vs rolling
  baseline, p99 tracked**; >10% p50 regression on any `bench:baseline`/
  `bench:matrix` metric fails the nightly and requires a ledger entry.
  >5% is a soft flag on the PR delta table.
- **Regression ledger:** `docs-internal/node-stdlib-regression-ledger.json`
  — schema `{bench, metric, delta, cause, disposition: fix|accepted,
  issue, approver}`; approver = runtime lead; cutover blocks on zero
  `disposition: fix` entries open and zero unexplained deltas.
- **Post-deletion story (flag gone):** rollback is release-level only.
  Late-found regressions follow optimize-forward with an SLO (regression
  acknowledged ≤1 day via nightly gate, fix-or-accepted-entry ≤1 week,
  owner: runtime lead); catastrophic regressions revert the release train.

## 12. Migration, cutover, deletion

### 12.1 Deletion inventory (review-corrected; native definition of "done")
**Deleted:**
- `packages/build-tools/bridge-src/**` — 30,486 LOC TS (41 files), its
  esbuild pipeline `packages/build-tools/scripts/build-v8-bridge.mjs`, the
  `v8-bridge-zlib.js` split, and `crates/build-support/v8_bridge_build.rs`.
- In `crates/execution/src/javascript.rs` (7,826 lines total): the embedded
  builtin JS (`node:stream`/`node:readline` minis and shim sources) and the
  legacy builtin-resolution surface that the real loader supersedes.
- In `crates/execution/src/node_import_cache.rs` (11,394 lines total) —
  **split, not wholesale** (python.rs and wasm.rs build launches on
  NodeImportCache; `NODE_WASM_RUNNER_SOURCE` lives here): delete only the
  JS-guest loader-hook/builtin surface (`loader.mjs`/`runner.mjs`
  templates, os/tls/vm shims, `BUILTIN_ASSETS`/`DENIED_BUILTINS`); the
  surviving module (rename: `runner_asset_cache.rs`) owns asset
  materialization, `NODE_IMPORT_CACHE_ASSET_VERSION`, and the WASM/Python
  runner sources.
- `crates/execution/assets/undici-shims/**` (**17 .js files +
  package.json**), `crates/execution/assets/polyfill-registry.json`, legacy
  `AGENTOS_ALLOWED_NODE_BUILTINS` host-node path.
- CJS export-name extraction + CJS shim generation in
  `crates/v8-runtime/src/execution.rs`.
- Browser runtime sources and its polyfill generators are **kept but disabled,
  not migrated** (Decision 4, §12.3). They are removed from CI/release/publish
  matrices in M0 and may cease to build after native `bridge-src/**` deletion;
  browser buildability is not a gate for this program.
- The RustCrypto-backed `_crypto*`, host TLS-upgrade, and
  `rustls-native-certs` bridge globals/handlers (Decision 2) —
  protocol-lockstep removal at M5, after shared OpenSSL-wasm is the only
  crypto/TLS path.
- Collateral retargeted or deleted with owners named in the milestone PRs:
  `scripts/verify-check-types.mjs` undici-shims reference; native-sidecar
  suites keyed to the legacy layer (`builtin_conformance`,
  `builtin_completeness`, `promisify_module_load` — these become
  real-stdlib gates where they encode guest-visible behavior, deleted where
  they encode bridge-src internals); `limits-inventory.json` bridge-src
  entries; normative text in `crates/CLAUDE.md` + `crates/execution/
  CLAUDE.md` (docs-in-same-change).
**Keeps:** WASI/Python runners, pyodide assets, bridge-contract globals (the
substrate), `~/.agents/recovery/secure-exec/` porting stops.

### 12.2 Cutover mechanics
- **Flag:** `AGENTOS_JS_STDLIB=real|legacy`. Policy note (review): repo
  CLAUDE.md forbids execution-mode flags that fall back to host execution —
  this flag is defensible **only because both values run in V8 isolates**;
  stated explicitly here and in the flag's doc. Flag removal is a hard M5
  exit criterion (owner: runtime lead). Post-deletion rollback is
  release-level (§11.3). While the flag exists, the snapshot cache warms
  per flavor (coldstart budget accounts for it).
- Sessions are homogeneous (no per-module runtime mixing) — with the single
  sanctioned, expiring M0→M1 loader exception (§5). Migration granularity
  lives in the suite ledger. The default flips only after the real path
  contains the measured legacy supported-test set and meets the same-day
  legacy performance gate (Decision 7, §§8.3, 10.1, 12.5).

### 12.3 Browser runtime (DECIDED — Decision 4: disabled, retained)
- Keep `packages/runtime-browser/**` and browser-specific generators in the
  repository; do not delete or migrate them in this program.
- In M0, remove browser build, test, publish, and release jobs from the active
  matrices. Browser failures cannot block CI or release, and the browser
  package is not shipped while disabled.
- Native cutover may delete shared legacy inputs the disabled browser source
  still references. This program makes no browser-buildability promise and
  carries no browser profile/design-doc deliverable. Re-enabling the browser
  runtime requires a separately approved migration and restores its own CI and
  release gates then.

### 12.4 Client-visible changes (review-corrected: NOT "none")
- `allowed_node_builtins` denial error shape/timing changes (§5): lockstep
  same-change updates to Rust client (`crates/client/src/config.rs`), TS
  options schema, and docs; TS/Rust behavioral identity preserved.
- **Native V8 platform tiering (not the disabled browser runtime):** public `jsRuntime` option maps to
  `AGENTOS_JS_PLATFORM` (`javascript.rs:3186-3227`) which today
  *subtractively scrubs* process/Buffer/require per execution. Real node's
  bootstrap makes `process` load-bearing — subtractive scrubbing does not
  transfer. Spec: preserve the existing public option values as
  **context-build bootstrap profiles**; M0 captures a global-surface fixture
  for every current value, and implementation must preserve that contract
  while the full-node value runs node bootstrap. This concerns native V8
  sessions only and does not reintroduce browser-runtime scope. Guest-visible
  behavior change → docs + both clients in the same change.
- Everything else (ACP/protocol, bridge globals) unchanged; bridge
  *additions* follow normal protocol-lockstep rules.

### 12.5 Done means
1. Native deletion inventory empty on `main`; the retained, disabled browser
   runtime is explicitly outside it (Decision 4); 2. the real node-suite set
   includes every legacy-passing test except explicit non-goals, and no
   `fail-accepted` lacks an issue link; 3. §8.3 legacy-parity and native
   budgets met with published same-machine
   before/after (§11.2); 4. docs + client schemas current; 5. vendor
   manifest + upgrade runbook in `docs-internal/`; 6. flag removed.

## 13. Phasing

**Cross-workspace entry gate:** before M0 implementation begins, finish and
forklift the ordered reg-tests handoff work: unskipped git-over-SSH clone/push,
then the procps-driven `/proc` completion. Read the current state and proof
requirements in `docs-internal/registry-networking-handoff.md` in the
`reg-tests` workspace. Do not move either workspace's `@` to inspect the other.

Cutover track:

| M | Scope | Acceptance |
|---|---|---|
| **M0** | Vendor pinned Node 24 LTS JS + leaf-lib sources; create the shared `toolchain/c/openssl` manifest/build/patch layout (§9.4) and Node adapter skeleton without a private OpenSSL copy; binding-grep reconcile vs the v26 POC; `crates/node-stdlib` skeleton; snapshot integration (flavor-keyed cache, defined fallback); **every pinned-v24 binding + process object loads inert** (every public module `require`s clean); interim loader exception documented; CPED build-flag verified; §11.2 measurement deliverables (legacy + real floors, dual-target runner, A/B + PR-delta tooling, snapshot + wasm microbenches); budgets restated as floor×headroom; suite harness + exact legacy/real set-diff ledger; **shared OpenSSL build spike (§7.4)**; **`toolchain/` lands on main + sysroot CI cache (§9.4)** + policy text on main (§9.5); **browser runtime removed from active CI/release/publish matrices but source retained (§12.3)** | real-stdlib session boots eager set from snapshot; `require` of all public modules succeeds; exact legacy/real ledger + bench baselines published; shared OpenSSL `libcrypto.a`/`libssl.a` compiles, emits a reproducibility manifest, and completes an in-guest handshake; toolchain builds green on main; browser cannot block CI/release |
| **M1** | fs complete: fd-level 1:1 bridge, base64-kill + backing-store transport (A/B in acceptance), stat truth, statfs/access/utimes/mkdtemp/copyFile/opendir, uv errno fixture, blob, permission row final; kernel readdir-ENOENT fix; GC/detach stress test; real CJS loader for user code (module_wrap minimal + compileFunctionForCJSLoader) — loader exception expires | `test-fs-*` sync+async ledger green (watch excluded); fs micro within budget; transport A/B published |
| **M2** | Event loop phases + HandleRegistry + explicit-microtask policy + MakeCallback discipline + loop-turn clock; async_wrap/async_context_frame HOST (ALS works); task_queue HOST pieces (unhandledRejection); messaging (DOMException, transferables, structuredClone); streams at suite depth; fs.watch kernel primitive + `fs_event_wrap`; limits table live | streams/timers/ALS suite categories green; ordering parity ledger clean; limits enforced with typed errors |
| **M3** | net (StreamBase contract incl. sync-write fast path, streamBaseState, accepted-handle hydration; kernel readiness push + accept events), dns/dgram, tty (guessHandleType fixture); http via llhttp-wasm + real `_http_*` + real undici fetch | net/http suite categories green (native ledger); http RPS/p99 + stream-throughput budgets met |
| **M4** | crypto + TLS adapters over the **one shared OpenSSL 3.5.x wasm build (Decision 2)**, real `versions.openssl`, entropy host import, shared VM CA; migrate registry C consumers and delete per-tool TLS adapters; zlib/brotli/zstd (wasm, new encoder rules); child_process + **guest `node` command / self-spawn (Decision 3)**; os/process_methods, sqlite, nghttp2-wasm | respective categories preserve the complete legacy-passing set and ratchet native passes (§10.1); cross-runtime shared-TLS e2e green; crypto-blob + lazy-instantiation budgets met |
| **M5** | ESM loader + vm/contextify completeness; native-V8 platform-tiering bootstrap profiles; flag default→real only after legacy functional/perf parity, then **flag removed**; **native deletion inventory executed** (incl. split-file surgery + `_crypto*`, host TLS, and host trust-store removal §12.1); bridge-call census (§8.2); final same-machine before/after report; docs/clients lockstep | §12.5 "done" |

Browser (Decision 4): source is retained but disabled from CI, release, and
publish in M0. It has no migration deliverable or acceptance gate in this
program and cannot block any milestone.

Parallel libc track (does not gate M5): **L1** eventfd (kernel fd object +
host import + pollable + conformance), **L2** epoll (libc emulation over
poll, level-triggered), **L3** threads-wall documentation. Landed when
ready; consumed by future registry-software work.

## 14. Risk register

| Risk | Exposure | Mitigation |
|---|---|---|
| Event-loop fidelity under net (phase-observing code) | M3 | POC de-risks fs/streams; phase queue + MakeCallback discipline land M2 before net; ledger catches encoded orderings |
| StreamBase sync/async duality subtleties | M3 | contract specced from `stream_base_commons.js`/`stream_wrap.cc`; errno fixtures; sync fast path in design not retrofit |
| Bridge chattiness dominates fs/net/loader perf | M1/M3/M5 | floors measured M0; backing-store transport; batching; census at M5 entry; ratio-gated nightly |
| CPED flag absent from our V8 build | M0 | verify first (M0 task); fallback = SetPromiseHooks path (slower, HOST already specced) |
| Shared OpenSSL 3.5.x wasm build + binding scale (Decision 2) | M0–M4 | edgejs build recipe and the proven mbedTLS/sysroot/socket/CA plumbing as starting points; inventory pinned-v24 binding surface; build + handshake spike in M0; lazy instantiation + blob budget; one content-addressed build and cross-runtime e2e prevent backend drift |
| Suite harness realism (Flags emulation, common/ deps) | M0+ | quantified up front (§10.1); Decisions 2+3 remove the two biggest self-skip categories; honest denominator per milestone |
| Snapshot bloat / lazy-compile latency | M0+ | size/build-time/lazy-compile metrics + budgets from M0; code-cache follow-up |
| Toolchain-on-main CI cost (Decision 1) | M0+ | content-hash sysroot cache; cold rebuild off the PR path; pins make cache correct by construction |
| Node-version upgrades churn binding ABI | post-cutover | pinned vendor manifest; upgrade runbook = suite ledger diff |
| Sysroot/artifact supply chain (unpinned inputs) | M0 | §9.4 pin-everything + content-hash metadata |
| Legacy/real dual maintenance drags | M1–M5 | legacy frozen (bugfix-only) at M1; flag removal is M5 exit |

## 15. Resolved questions and implementation decision gates

No user product decision remains open as of 2026-07-09. §0 records the
answers: Node 24.15.0 LTS; one real shared OpenSSL 3.5.x wasm backend with in-guest
TLS; workers unsupported; empirical legacy functional/performance parity;
dependency-driven sequencing; browser source retained but disabled and wholly
outside this program.

The following are engineering gates, not user preference questions:
1. M0 pins the exact Node 24 tag and reconciles the binding inventory.
2. M0 proves the shared OpenSSL build + handshake. A failure is escalated only
   with concrete sysroot/OpenSSL-patch evidence (§7.4), never by silently
   adding another TLS backend.
3. M0 measures exact legacy/real suite sets and benchmark floors; those
   measurements replace guessed denominators and provisional budgets.
4. URLPattern uses JS-RegExp wasm imports or a HOST provider based on the M0
   prototype; either must pass the same native-node differential fixtures.
5. nghttp2-wasm is the M4 path; a host fallback requires a measured,
   documented blocker (§4).

---

## Appendix A — Review dispositions

44 findings from 4 reviewers. **Accepted: 41 · Accepted-with-modification:
3 · Rejected: 0.** Every finding listed.

Reviewer A (node internals):
- A1 worker STUB bricks bootstrap — **accepted**: STUB policy rewritten to
  inert-data (§4); worker row reclassified; M0 inert-load gate added.
- A2 async_wrap/ACF need CPED/HOST — **accepted**: reclassified HOST; CPED
  build-flag verification is an M0 task; MakeCallback discipline §6.4.
- A3 M1 unreachable / all-bindings-at-M0 / interim loader — **accepted**:
  M0 gate "all bindings load inert" (§4); CJS loader pulled to M1 with a
  sanctioned expiring M0 exception (§5); milestones resequenced (§13).
- A4 permission binding missing — **accepted**: row added (§4), counted.
- A5 task_queue not pure JS — **accepted**: JS+HOST; microtask policy §6.3.
- A6 suite realism (execPath/openssl/Flags/fraction) — **accepted**: §10.1
  quantified; §10.5 added; old OQs 7/8/9 added (7 and 8 since resolved, §0).
- A7 process object below the seam — **accepted**: §2 principle 2 amended;
  §4.5 process-object synthesis section added.
- A8 messaging underscoped — **accepted**: row expanded JS+HOST (§4);
  port-drain macrotask class added to §6.1.
- A9 StreamBase sync/async duality — **accepted**: §7.3 rewritten.
- A10 config pins/property-readable stubs — **accepted**: config row + STUB
  policy (§4).
- A11 crypto 174-prop gate — **accepted**: §4 crypto row + §7.4 + M4
  acceptance rephrased.
- A12 snapshot external references for HOST — **accepted**: §3.3; JS/HOST
  split planned at M0.
- A13 getLibuvNow loop-cached — **accepted**: §6.5.
- A14 util row omissions — **accepted**: row expanded incl. guessHandleType
  as BRIDGE + fixture.
- A15 nightly-vs-tag grep reconcile — **accepted**: §3.1 M0 item.

Reviewer B (architecture/security):
- B1 node_import_cache wholesale delete breaks python/wasm — **accepted**:
  §12.1 split; surviving module named (`runner_asset_cache.rs`).
- B2 denial-at-source unsound / kernel is boundary — **accepted**: §5
  rewritten (post-restore per-execution gate, undeniable bootstrap set,
  API-tiering framing); "strictly stronger" claim withdrawn.
- B3 browser runtime consumers — **accepted**: §12.3 added; old OQ 5
  (since resolved by Decision 4, §0); deletion
  gate scoped.
- B4 snapshot mechanics misdescribed — **accepted**: §3.3 rewritten (lazy
  subprocess, cache keying + flavor, wasm-not-serializable → post-restore,
  degrade path replaced by defined-fallback-or-typed-error).
- B5 async zero-copy data race — **accepted**: §8.1 rewritten
  (copy-at-dispatch on V8 thread; validation; bounded prealloc).
- B6 unbounded new collections — **accepted**: §2 principle 6, §6.6, limits
  in M2 acceptance.
- B7 client impact not "none" — **accepted**: §12.4 added (allowlist
  lockstep; platform tiering → bootstrap profiles).
- B8 inventory collateral — **accepted**: §12.1 collateral list.
- B9 flag policy/rollback/warmup — **accepted**: §12.2 + §11.3
  post-deletion story + snapshot warmup note.
- B10 cheap-gate tier conflict — **accepted**: §10.1 CI tiers corrected.
- B11 undici-shims count — **accepted**: 17 + package.json (§12.1).
- B12 policy text not on main — **accepted**: §9.5 M0 prerequisite.

Reviewer C (toolchain/libc):
- C1 brotli/zstd encoder gap — **accepted**: §4 zlib row + §7.4 (new build
  rules; reuse claim withdrawn).
- C2 publish analogy false / sysroot CI — **accepted-with-modification**:
  §9.4 rewritten with the recommended prebuilt-artifact path and costs of
  both; final call stayed with the user as old OQ 4 (since resolved by
  Decision 1, §0 — toolchain in the monorepo),
  per coordinator instruction to keep genuine user decisions open.
- C3 leaf-lib runtime location — **accepted**: §3.2 (build.rs sha256 fetch
  → OUT_DIR, typed error, docker/darwin byte-identity).
- C4 url_pattern regex provider — **accepted**: §4 row split (JS/HOST or
  wasm-with-JS-regex-imports; plain ada-wasm forbidden).
- C5 eventfd/epoll off critical path — **accepted**: §9 restructured as
  parallel L-track; removed from M5 gate.
- C6 eventfd/epoll layering — **accepted**: §9.1/9.2 (kernel fd object +
  host import, pollable, eventfd-in-epoll conformance; epoll as libc
  emulation first).
- C7 unpinned toolchain inputs — **accepted**: §9.4 pin-everything +
  content hash.
- C8 wasm-opt nondeterminism — **accepted**: §9.4 (pinned binaryen or drop;
  CI=local).
- C9 llhttp generated-C / vendor from tag — **accepted**: §3.2 vendor/native
  from pinned tag.
- C10 import-object contract — **accepted**: §9.4 (fd_write→log, trap
  typed, minimize).
- C11 ICU conflation — **accepted**: §4.6 added; old OQ 5 resolved
  (hasIntl:false shipped; degradation enumerated); OQ slot reused for
  browser.

Reviewer D (perf/benchmarks):
- D1 gate thresholds unenforceable — **accepted**: §11.3 rebuilt (2.0× PR
  gate stays; 10% p50 nightly on pinned-hardware-or-ratios; statistics
  named).
- D2 two-harness reality / wrong commands — **accepted**: §11.1 rewritten;
  runtime-benchmarks authoritative; environment-keyed paths.
- D3 budgets without floors — **accepted-with-modification**: §8.3 keeps
  provisional numbers but is explicitly restated as floor×headroom pending
  the M0 floor measurements (numbers can't precede the instrument runs; the
  M0 amendment replaces them).
- D4 native comparison harness missing / fairness — **accepted**: §11.2
  dual-target runner + tmpfs fairness protocol as M0 deliverable.
- D5 transport budget vs phasing contradiction — **accepted**: both bounds
  tied to the M1 transport A/B (§8.3, §13 M1).
- D6 ledger/A-B/post-deletion vague — **accepted**: §11.3 (ledger path,
  schema, approver; A/B + PR-delta job in M0; optimize-forward SLO +
  release-level revert).
- D7 coldstart gate blind to lazy compile — **accepted**: §11.2 snapshot
  metrics + fs/http coldstart variant; size budget §8.3.
- D8 wasm lifecycle unmeasured — **accepted**: §8.4/§11.2 named microbench;
  sharing reclassified as assumption-to-validate.
- D9 frozen-baseline methodology — **accepted**: §11.2 (M0 freeze =
  early-warning; published before/after = same-machine same-day A/B).
- D10 stale cross-references — **accepted**: all §-refs renumbered
  (conformance=§10, benchmarks=§11).
- D11 backing-store API misnamed — **accepted**: §8.1 corrected
  (`get_backing_store().data()`), GC/detach stress test in M1.
- D12 missing macro-bench budgets — **accepted**: §8.3 stream-throughput +
  import-storm budgets; §8.2 M5-entry census.

Accepted-with-modification rationale summary: C2 and D3 keep a user OQ /
provisional numbers respectively where the reviewer asked for hard
resolution — in both cases because the resolving input (user repo-strategy
preference; M0 floor measurements) does not exist yet; the spec now says
exactly how and when each gets resolved. No finding was rejected.

**Post-review update (v4):** the C2 open question was subsequently resolved
by the user (Decision 1, §0 — toolchain in the monorepo), superseding the
prebuilt-artifact recommendation; the reviewer's caching and pinning
requirements are retained in full in §9.4. The final §0 decisions also
supersede the v2 recommendations: crypto/TLS now use one real shared OpenSSL
3.5.x wasm build, browser source is retained but disabled and removed from
this program, self-spawn is adopted, Node is pinned to v24.15.0 LTS, workers
are unsupported at cutover, and functional/performance acceptance is measured
against the legacy implementation rather than a guessed suite fraction.

---
*Grounded against: `crates/bridge/bridge-contract.json` (178 globals),
`crates/v8-runtime/src/{session.rs,snapshot.rs}`, `crates/execution/src/
{javascript.rs,node_import_cache.rs,python.rs,wasm.rs}`, bridge-src LOC,
`packages/runtime-benchmarks/*`, `scripts/benchmarks/*`,
`.github/workflows/{bench.yml,publish.yaml}`, `~/misc/node` @ fbf82766d62
(69 internalBindings + test/parallel census), toolchain/ Makefile +
patch-wasi-libc.sh, POC stages 1–3 artifacts, and the 4-lens review
(2026-07-09T14-20-00PST-spec-review-findings.md).*
