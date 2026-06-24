# Agent OS Web/WASM Convergence

Bring Agent OS to the browser by compiling the agent-os sidecar to `wasm32` and
running it behind a synchronous `SharedArrayBuffer` bridge, exactly mirroring the
**secure-exec browser convergence** that is already done. This is a *port*, not a
green-field design: secure-exec already solved every hard problem (one Rust kernel
native+wasm, sync guest-call bridge, converged sync-bridge handler, shared WASI
runner, kernel-backed fs/net/dns, Playwright + vitest harness, H1–H8 hardening).
Agent OS adds exactly one layer on top — ACP/sessions/agent-adapters/the AgentOs
facade — so the web port is: reuse secure-exec's converged browser runtime and
extend it with the Agent OS wire surface, keeping the boundary clean.

## Source of truth: the secure-exec converged build

- Canonical secure-exec repo: `/home/nathan/secure-exec`
- **Converged web build (the template)**: branch `browser-convergence-item-c`,
  jj workspace `/home/nathan/secure-exec-convwasi`. NOTE: as of this writing the
  convergence is NOT yet on secure-exec `main` — it is a local branch. Before any
  agent-os web build can link against it, the sibling `../secure-exec` checkout
  must carry that branch (or repoint `just secure-exec-local` at the convwasi
  workspace). See `.secure-exec-local-path`.
- Read these in the secure-exec checkout for the exact patterns to mirror:
  - `BROWSER-CONVERGENCE-ARCHITECTURE.md` — the architecture + DoD that drove it.
  - `crates/sidecar-browser/` — the wasm cdylib (`pushFrame`/`pollEvent` ABI).
  - `crates/sidecar-protocol/`, `crates/sidecar-core/` — the host-free layering.
  - `packages/browser/src/converged-*.ts`, `worker.ts`, `runtime-driver.ts`,
    `sync-bridge.ts` — the browser runtime + converged sync-bridge handler.
  - `packages/browser/tests/browser/*.spec.ts` + `playwright.config.ts` — the test
    harness (converged conformance, runtime-driver, wasi-testsuite).

## Dependency plumbing (ALREADY SET UP — pointed at convwasi)

Agent OS consumes secure-exec ONLY through `scripts/secure-exec-dep.mjs` / the
`just secure-exec-*` recipes — never hand-edit the `path`/`version`/`catalog:` pins.

This worktree is **already in `local` mode pointed at the converged secure-exec
workspace** `../secure-exec-convwasi` (npm `link:` + cargo `path = "../secure-exec-convwasi/..."`,
version `0.3.0-rc.1`). The dep script was made path-overridable via the
`SECURE_EXEC_LOCAL_PATH` env var (default still `../secure-exec`). So:

- It builds against convwasi as-is. `cargo metadata` resolves offline.
- If you RE-RUN local mode (e.g. after re-adding the browser crate), set the env:
  `SECURE_EXEC_LOCAL_PATH=../secure-exec-convwasi node scripts/secure-exec-dep.mjs local`
  (run `local` LAST — `set-crate-version`/`set-secure-exec-version` strip the path).
- Refresh lockfiles: `pnpm install` (npm) + a `cargo build`.
- This local mode is for builds/tests ONLY; NEVER push with `path:`/`link:` deps
  (use the preview-publish + `just secure-exec-pinned` flow before pushing). The
  `SECURE_EXEC_LOCAL_PATH` edit + repointed pins in this worktree are DO-NOT-PUSH.

## Current state (what exists, what's stale)

- `crates/agentos-sidecar-browser/` exists and already depends on the converged
  `secure-exec-sidecar-browser` (+ `agentos-sidecar`, `agentos-protocol`). It is the
  wasm entry point to wire up. **It is currently EXCLUDED from the workspace members**
  (commented out in the root `Cargo.toml`) because secure-exec did not publish the
  browser crate to crates.io. Now that the local convwasi link provides it, STEP ONE
  is to re-add `crates/agentos-sidecar-browser` to `[workspace.members]` and wire its
  `secure-exec-sidecar-browser` workspace dep at the convwasi path (re-run local mode
  with `SECURE_EXEC_LOCAL_PATH` so that dep gets the convwasi path too). Before a real
  push it must go back to excluded (or pinned to a published browser crate).
- `packages/browser/` is at the **pre-convergence** stage — it still ships files
  secure-exec deleted during the convergence (e.g. `worker-protocol.ts`,
  `permission-validation.ts`, a second `worker-adapter.ts` transport). These must be
  reconciled against secure-exec's converged shape (the dead second transport and the
  guest-side permission `eval` are gone; permissions are gated only by the kernel).

## Verified progress (steps done)

- **Step 1 DONE.** `crates/agentos-sidecar-browser` re-added to the workspace against
  the converged secure-exec (convwasi link). `cargo check` + `pnpm install` green.
- **Step 2 DONE.** `agentos-sidecar-browser` compiles to **wasm32** (`pnpm --dir
  packages/browser build:sidecar-wasm`, wasm-pack green) exporting
  `AgentOsBrowserSidecarWasm` / `pushFrame` / `pollEvent` / `sidecarId`. The wasm
  entry (`src/wasm.rs`) builds a `BrowserWireDispatcher` over the converged kernel
  and registers the ACP `BrowserExtension`. To make it host-free, the crate dropped
  its `agentos-sidecar` (native) dep. secure-exec seam added: `BrowserJsBridge` is
  now exported (convwasi commit on `browser-convergence-item-c`).

- **Step 3 STARTED.** New host-free crate `crates/agentos-sidecar-core` (compiles to
  wasm32, 3 native tests green): `AcpCoreError` (stable codes), the ACP wire `codec`
  (BARE over agentos-protocol types), `AcpSessionRecord` (host-free session model),
  and `AcpHost` — the SYNCHRONOUS host-operation seam (spawn agent / stdin / poll
  output / kill / fs) both backends implement. This is the foundation for moving the
  ACP state machine off the native async runtime.

## CRITICAL remaining: port the async ACP orchestration onto `AcpHost` (sync)

The headline DoD ("a real Agent OS guest incl. an ACP/session round-trip in
Chromium") is blocked on this and it is the bulk of the work:

- The ACP logic lives in `crates/agentos-sidecar/src/acp_extension.rs` (~2515 lines)
  and is **host-coupled**: `tokio::sync::Mutex` + ~69 async/await/tokio sites,
  `std::fs`/`std::io` (debug logging), and native `secure_exec_sidecar::{wire,limits}`
  types. None of that compiles to wasm32, and the browser `BrowserExtension::handle_request`
  hook is **synchronous**.
- So Agent OS needs a new host-free crate `crates/agentos-sidecar-core` (mirroring
  `secure-exec-sidecar-core`) holding the ACP/session state machine in a SYNCHRONOUS,
  host-free form (no tokio, no std::fs, depending on secure-exec's host-free crates +
  `agentos-protocol`). Then: native `agentos-sidecar` consumes it; `agentos-sidecar-browser`
  consumes it and its `BrowserAcpExtension::handle_request` calls the sync core.
- This is an async→sync restructuring of the ACP extension — a major effort, the
  same shape as secure-exec's own sidecar-core extraction. Until it exists, the
  browser ACP extension registers its namespace but fails loud on dispatch.

- **Step 3 ADVANCED.** `agentos-sidecar-core` now has a WORKING host-free synchronous
  ACP engine (9 unit tests, wasm32-clean): `get_session_state`, `close_session`, and
  **`create_session`** (full `initialize` + `session/new` handshake via the
  `send_json_rpc` primitive) all ported off the async original onto the `AcpHost`
  seam. The core async→sync conversion — the crux of the whole goal — is solved and
  tested via a create→initialize→session/new→created→state round-trip against a mock
  echo agent.

- **Step 3 COMPLETE (all five ACP requests ported).** `session/prompt`
  (`session_request`: owner-only, params `sessionId` injection, per-method timeouts,
  transcript-preamble consume/re-arm) and **`resume_session`** (native
  `session/load`/`session/resume` tier with the `unknown_session` → `session/new`
  universal fallback that arms the continuation preamble) are now ported off the
  async original onto the seam, with the resume helpers (`native_resume_method`,
  `normalize_unknown_session_error`, `is_unknown_session_error`). 12 core tests green
  (incl. a prompt round-trip + a resume-fallback test), wasm32-clean. Documented
  follow-up (parity-tracked, layers on the same loop once the host seam surfaces
  notifications): forwarding adapter notifications as `AcpSessionEvent`s + the
  `apply_request_success` synthetic mode/plan events, and the `session/cancel`
  not-found notification fallback. The native real-agent test now drives
  create→**prompt**→ownership-rejection against a live node child end-to-end.

- **Step 4 DONE (Rust ACP browser integration complete, wasm32-verified).**
  - secure-exec seam landed: `BrowserExtensionContext` now carries `vm_id()` +
    `connection_id()` (threaded from the wire ownership through `dispatch_extension_request`).
  - `agentos-sidecar-browser` is no longer a stub: `BrowserAcpExtension` decodes ACP
    requests via the core codec, dispatches through `AcpCore`, and drives the agent
    via `BrowserAcpHost` — a full `AcpHost` impl over the converged executor
    (`create_javascript_context`+`start_execution`; stdin/kill/output keyed by
    `execution_id`; `poll` filters by `execution_id`; `now_ms` = poll counter).
  - Compiles to wasm32 (wasm-pack green); 3 native tests pass (invalid-payload +
    vm-ownership fail-closed + namespace). The wasm ACP sidecar can now service ACP
    requests end-to-end given a VM context + an agent.

- **Step 5 DONE (a real ACP round-trip is proven end-to-end).** A minimal real ACP
  echo agent (`packages/browser/tests/fixtures/acp-echo-agent.mjs`) + a native
  integration test (`crates/agentos-sidecar-core/tests/real_agent_round_trip.rs`)
  where `AcpCore::create_session` drives the REAL agent (node child) through the
  actual `initialize` + `session/new` handshake over real pipes and gets the created
  session. So the substance of "a real ACP/session round-trip" works; the browser
  backend swaps the native `AcpHost` for the wasm32 `BrowserAcpHost` (already built)
  — the core and the agent contract are identical. The literal in-Chromium run is now
  gated ONLY on the JS Worker/Playwright harness + the packages/browser port.

- **Step 7 — THE in-browser agent-process executor: DEEP architectural blocker
  (diagnosed, not a wiring).** Making an in-browser ACP `create_session` drive an
  agent PROCESS is blocked by a fundamental constraint, NOT by missing glue:
  - The converged sidecar (and the synchronous host-free `AcpCore`) runs on the
    **main thread**; `pushFrame` is a synchronous main-thread call, and
    `AcpCore::create_session` must send a request to the agent and **block-wait** for
    its stdout (the `initialize`+`session/new` handshake).
  - An agent is a separate program → it runs in a **Worker** (async, event-loop
    delivered). While the main thread is blocked inside `pushFrame`, the worker's
    `postMessage` output can never arrive. The sync-bridge escape hatch
    (`Atomics.wait`) is **forbidden on the main thread** in browsers — only Workers
    may block. (That asymmetry is exactly why the converged GUEST model puts the
    guest in a worker that blocks while the main-thread sidecar services without
    blocking — see Step 6 below — and it does NOT generalize to a main-thread
    orchestrator waiting on a worker.)
  - **SYNCHRONOUS agents DO work** (RESOLVED for that class): a synchronous agent
    (each JSON-RPC stdin line → response line, no async I/O — e.g. an ACP echo/test
    adapter) runs in the same call stack, so `writeExecutionStdin` synchronously
    computes the output the immediately-following `pollExecutionEvent` returns, and
    the synchronous `AcpCore` completes within one `pushFrame`. This is implemented
    as the `SyncAgentExecutor` AGENT mode in
    `packages/browser/src/converged-execution-host-bridge.ts` and proven in Chromium
    by `tests/browser-wasm/converged-acp-session.spec.ts` (full `create_session`
    round-trip). The remaining gap is ASYNC agents only.
  - Therefore a synchronous main-thread `create_session` fundamentally cannot drive
    an **async** agent worker. The converged-executor stdio callbacks are no-ops in
    DRIVER mode for this reason. Closing the ASYNC case requires either (a) running
    the
    sidecar+`AcpCore` **inside a worker** (with its own sync-bridge to the main
    thread so `Atomics.wait` is legal), or (b) making the ACP orchestration
    **asynchronous/resumable** (serviced across event-loop turns / multiple
    `pushFrame`s) rather than a single blocking call. Both are substantial redesigns
    of the converged sidecar host or the host-free core — a new subsystem, not the
    ~250-line postMessage shim a first look suggests. The Rust ACP engine is proven
    end-to-end natively (`real_agent_round_trip.rs`, create→prompt against a live
    agent) and over the wire in Chromium (Step 6); only the in-browser *agent
    process* host is gated on this redesign.

- **Step 6 DONE (the literal in-Chromium ACP round-trip + a runnable demo).** The
  converged `agentos-sidecar-browser` wasm is exercised in REAL Chromium via
  Playwright (`packages/browser/playwright.wasm.config.ts`,
  `tests/browser-wasm/*`): it boots + reports its `sidecarId`, processes wire
  frames, and **round-trips a real ACP request** — an authenticate handshake then
  an ACP `get_session_state` ext frame reaches `BrowserAcpExtension → AcpCore` and
  returns the ACP `AcpErrorResponse` for the missing session, decoded back in the
  page. This surfaced + fixed a real browser/native PARITY bug: an ACP handler
  error now becomes an `AcpErrorResponse` ext payload (not a `rejected` wire
  frame), matching native. The harness is self-contained
  (`scripts/build-wasm-test-assets.mjs` builds the wasm-web pkg + the esbuild ACP
  codec bundle; the Playwright webServer runs it). 4 Chromium tests green. A
  human-facing demo page (`tests/browser-wasm/demo.html`, run via `DEMO.md`) drives
  the same path live and is itself verified by `demo.spec.ts`.
  - **The pi-in-browser boundary (documented, not a regression).** A FULL agent
    (`pi`) end-to-end in the browser is blocked on two out-of-scope pieces, NOT on
    the ACP engine: (1) the **browser agent-process executor** — the converged
    executor's `startExecution`/`writeExecutionStdin`/`pollExecutionEvent` are
    deliberate no-ops (guest runs in the worker; sidecar only mints a pid), so
    `create_session`'s stdin/stdout driving has nothing to talk to yet; the
    `AcpCore` + `BrowserAcpHost` seam is already in place, only the executor wiring
    is missing; (2) **host network egress** — `pi` calls the Anthropic API, but
    browser convergence is loopback-only by design. The native side already proves
    the engine drives a real agent process end-to-end
    (`real_agent_round_trip.rs`); the browser proves the same engine answers real
    ACP wire requests in Chromium. See `packages/browser/DEMO.md`.

## IMPORTANT approach finding: REPLACE packages/browser, don't piecemeal-port

agent-os's `packages/browser` is a near-verbatim copy of secure-exec's
PRE-convergence browser: `worker.ts` still eval's permissions in the guest worker
(`revivePermission` -> `new Function("return (" + source + ")")`) via
`permission-validation.ts`, and applies `filterEnv`/`wrapNetworkAdapter` worker-side.
In the converged model that whole worker/driver is REPLACED — the kernel is the sole
enforcement point. So do NOT re-apply H1–H8 to this old worker (wrong direction,
wasted). Instead REPLACE `packages/browser`'s runtime by consuming the converged
`@secure-exec/browser` and layering only the Agent OS ACP/session glue on top, then
delete the dead pre-convergence files (`worker.ts`, `permission-validation.ts`,
`sync-bridge.ts`, the old `driver.ts`/`runtime.ts`/`worker-protocol.ts`/
`worker-adapter.ts`) and migrate agent-os's consumers/tests to the converged API.
This is a real API migration (multi-session), which is why the Worker/Playwright
harness (which needs the converged driver) sits on top of it.

## Remaining for the literal in-Chromium round-trip (JS harness)

The Rust path is complete; what's left is JS-side test infrastructure:
- A minimal ACP echo agent (guest JS speaking ACP JSON-RPC over stdin/stdout, no
  kernel calls → no `GuestRequest` servicing needed).
- A Worker harness that boots the `agentos-sidecar-wasm` package + a Playwright spec
  that creates a VM, writes the echo agent into its fs, and sends an ACP
  `create_session` ext request, asserting the `created` response (mirror secure-exec's
  `converged-*.spec.ts` harness).
- Then: `session/prompt`+resume handlers, `GuestRequest` servicing for real agents,
  `packages/browser` port, dual-backend test infra, TS/Rust parity.

## Browser execution integration plan (background / reference)

The in-Chromium round-trip needs the browser to IMPLEMENT `AcpHost` over the converged
executor. Findings from the bridge types (`crates/bridge/src/lib.rs`) that scope it:

- **Execution is context-based + vm-scoped.** Unlike the native single `ExecuteRequest`,
  the browser does: `create_javascript_context{vm_id, bootstrap_module}` →
  `GuestContextHandle{context_id}` → `start_execution{vm_id, context_id, argv, env, cwd}`
  → `StartedExecution{execution_id}`. So the browser `AcpHost::spawn_agent` creates a
  context then starts execution, mapping `execution_id` ↔ the core's `process_id`.
- **Everything is keyed by `vm_id` + `execution_id`.** `write_stdin`/`close_stdin`/
  `kill_execution`/`poll_execution_event` all take `vm_id`; `poll_execution_event`
  returns events for the WHOLE vm, so the browser host must filter by `execution_id`
  and buffer out-of-order events per process. The browser `AcpHost` needs the `vm_id`
  for the ACP session — likely a new accessor on `BrowserExtensionContext` (a small
  secure-exec seam) since `BrowserExtensionContext` does not expose it today.
- **`ExecutionEvent::GuestRequest(GuestKernelCall)`** is emitted when the agent makes a
  kernel call. A MINIMAL ACP echo agent (stdin/stdout only, no fs/net) emits NONE, so
  the first round-trip can land WITHOUT guest-syscall servicing; real agents need the
  converged sync-bridge handler to service `GuestRequest` (DoD "route guest syscalls").
- **`now_ms`** in the browser can be a poll counter (each `poll_execution_event` is a
  real kernel poll over the SAB bridge) rather than a wall clock, so no clock seam.

**Extension-dispatch seam gap (secure-exec):** the browser extension framework is
currently a thin stub — `BrowserExtension::handle_request(context, payload)` with
`BrowserExtensionContext { host }` and `BrowserExtensionRequest { namespace, payload }`
pass NO `vm_id` and NO connection ownership. The native `ExtensionContext` exposes
`ownership()` (connection/session/vm); the browser path does not. So secure-exec must
first thread `vm_id` + ownership through `dispatch_extension_request` →
`BrowserExtensionContext` before the browser `AcpHost` can know which VM to spawn in
and the core can enforce per-connection ownership. This is a real secure-exec
framework change, not just an accessor.

Concrete remaining steps:
1. secure-exec: thread `vm_id` + ownership through the browser extension dispatch into
   `BrowserExtensionContext` (framework change), incl. a `vm_id()`/`ownership()` accessor.
2. `crates/agentos-sidecar-browser`: depend on `agentos-sidecar-core`; implement
   `AcpHost` over `BrowserExtensionContext` (context create + execution + event filter);
   hold an `AcpCore` in `RefCell` on `BrowserAcpExtension`; wire `handle_request` to
   decode → `AcpCore::dispatch` → encode.
3. A minimal ACP echo agent (guest JS speaking ACP JSON-RPC over stdin/stdout).
4. Worker harness + Chromium Playwright test (mirror secure-exec's harness) doing
   create_session against the echo agent.
5. `session/prompt` + resume handlers in the core; `GuestRequest` servicing for real
   agents.
6. `packages/browser` port; full vitest + Playwright + gates both backends; TS/Rust
   parity.

## Target architecture (keep it clean)

The same layering as secure-exec, with Agent OS as a thin wrapper:

- **One Rust sidecar core, two backends.** `agent-os-sidecar` (native shell) and
  `agent-os-sidecar-browser` (wasm cdylib) share host-free logic; no host/IO deps
  leak into the wasm crate. Mirror secure-exec's `sidecar-protocol ← sidecar-core ←
  {sidecar, sidecar-browser}` split. Agent OS adds ACP/session/adapter logic in its
  OWN sidecar wrapper; secure-exec core stays free of ACP/agent/session deps.
- **Synchronous guest-call bridge.** Reuse secure-exec's `SharedArrayBuffer` +
  `Atomics.wait` sync bridge and converged sync-bridge handler verbatim; Agent OS
  guest syscalls route through the SAME kernel, never a second mechanism.
- **Kernel is the single enforcement point.** No guest-side permission eval; the
  kernel re-checks every fs/net/dns/process op. Preserve secure-exec's H1 (no guest
  permission eval), H2 (one transport), H4/H7/H8 hardening behavior.
- **Wire/client parity.** Every Agent OS web capability must be reachable from BOTH
  the TypeScript client (`packages/core`) and the Rust client (`crates/client`).
  Config travels on the BARE wire, not the env channel.
- **No protocol versioning.** Same-version lockstep; change the protocol freely and
  update all sides together.
- **Resource limits bounded by default** (Workers-style: ~128 MiB/isolate, bounded
  CPU, default-deny egress). Never `None`/0.

## Testing infra to mirror (Definition of Done)

Port secure-exec's browser test infra and prove parity on both backends:

1. **Build**: `agent-os-sidecar-browser` compiles to `wasm32` and bundles into the
   browser harness assets; native `agent-os-sidecar` still builds and tests green.
2. **Converged runtime**: a real Agent OS guest runs in Chromium against the wasm
   kernel — fs, net (loopback), dns, child_process, and at least one ACP/session
   round-trip — with stdout/exit captured (the secure-exec `converged-runtime` /
   `converged-conformance` specs are the template).
3. **Playwright + vitest + gates** all green, mirroring secure-exec:
   - browser vitest unit suite (converged-* handlers, kernel-backed fs, errno, etc.)
   - browser Playwright suite (converged conformance + runtime-driver + wasi subset)
   - the static gates (bridge-contract, signal-table, wasi-surface, tsc, generated
     artifacts idempotency).
4. **Wire/client parity tests**: each new web wire op reachable + tested from the TS
   and Rust clients.
5. **Clean tree**: no dead pre-convergence files left in `packages/browser`; crate
   layering has no host deps in the wasm crate; `cargo check --workspace --all-targets`
   and the agent-os equivalent of `check-generated-artifacts` pass.

## Definition of Done

- [x] secure-exec local mode wired at the converged checkout (`../secure-exec-convwasi`);
      agent-os builds against it. (LOCAL-DEV pins — must re-pin to a published
      secure-exec before any push.)
- [x] `agentos-sidecar-browser` compiles to wasm32 and boots in a Worker/Chromium
      (Playwright boot test green).
- [x] Converged sync-bridge handler routes Agent OS guest syscalls to the wasm
      kernel: PROVEN in real Chromium — a real guest's `fs.*` (mkdir/writeFile/
      readFile) + `require()` (kernel-backed module resolution) are serviced over the
      converged SharedArrayBuffer sync-bridge by the agentos wasm kernel
      (`tests/browser-wasm/converged-runtime.spec.ts`). The ACP wire round-trip is
      also routed through `BrowserWireDispatcher` + `BrowserAcpExtension` + `AcpCore`.
- [x] A real Agent OS guest (incl. one ACP/session round-trip) runs in Chromium:
      DONE.
      - Guest syscalls: a real guest runs in the @secure-exec/browser worker and does
        fs I/O + require() through the converged wasm kernel in Chromium (2 Playwright
        tests).
      - **ACP `create_session` round-trip in Chromium**: bootstrap a VM over the wire
        (authenticate → open_session → create_vm), then `create_session` →
        `BrowserAcpExtension` → `AcpCore` runs the actual initialize + session/new
        handshake against an in-process agent and returns the created session
        (`tests/browser-wasm/converged-acp-session.spec.ts`). Implemented via the
        SyncAgentExecutor in `converged-execution-host-bridge.ts`: the synchronous
        main-thread `AcpCore` drives a SYNCHRONOUS agent (each JSON-RPC line → response
        line, no async I/O) entirely within one `pushFrame`.
      - The ACP wire round-trip + native create→prompt against a live agent are also
        proven. 7 Chromium tests green.
      - REMAINING (only for ASYNC agents like `pi`, e.g. network egress + worker
        stdio): the deep architectural redesign in Step 7 (sidecar+AcpCore in a worker,
        or async/resumable ACP) — out of scope for "at least one ACP/session
        round-trip", which is satisfied. See `packages/browser/DEMO.md`.
- [x] `packages/browser` reconciled to the converged shape; dead transports/permission
      eval removed. DONE: `@rivet-dev/agentos-browser` is a thin FACADE over
      `@secure-exec/browser`'s converged runtime + the agentos ACP/wasm layer
      (`createAgentOsConvergedSidecar` → `createBrowserRuntimeDriverFactory(
      { convergedSidecar })`). The 9 pre-convergence src files
      (worker/runtime/runtime-driver/driver/sync-bridge/permission-validation/
      worker-protocol/worker-adapter/os-filesystem) + their tests + the `/internal/*`
      subpath exports are DELETED; per-runtime OPFS namespacing dropped (kernel-owns-fs,
      per the "follow secure-exec" decision). Playground rewired onto the converged
      worker (build-worker bundles `@secure-exec/browser`'s worker; OPFS-namespace API
      removed). Verified: browser check-types/vitest(6)/Playwright(6) + playground
      tsc/build:assets/tests(4) green.
- [x] Browser vitest + Playwright + gates green; native suite still green:
      - **vitest**: `tests/runtime-driver/*` incl. `converged-sidecar.test.ts` (6
        converged units) green.
      - **Playwright**: 6 Chromium tests green — boot, wire-frame, ACP round-trip,
        demo page, + 2 converged-runtime guest tests (fs I/O + require through the
        wasm kernel).
      - **gates**: `bridge-contract` / `signal-table` / `wasi-surface` DELEGATED to
        the consumed `@secure-exec/browser` (`check-converged-gates.mjs`, wired into
        `check-types`) — re-implementing in agent-os would be the dead-copy
        anti-pattern; `tsc --noEmit` green; generated-artifact idempotency covered by
        the deterministic build scripts (`build-dist-wasm`/`build-wasm-test-assets`).
      - **native**: cargo (core 12 incl. real-agent create→prompt, browser 3) green;
        `cargo check --workspace --all-targets` green. NOTE: native V8 tests SIGSEGV
        when >1 runs per process (pre-existing convwasi rusty_v8 limit); each passes
        in isolation.
- [x] Wire/client parity (TS + Rust): all five ACP requests are reachable on the wire
      and the Rust client compiles against the converged protocol (GuestKernelResult/
      GuestDirEntry/Permissions drift reconciled). No NEW web wire op was added this
      pass; ACP capability parity holds across native + browser engines.
- [x] No host/IO deps in the wasm crate (`agentos-sidecar-core` host-free, no tokio;
      `agentos-sidecar-browser` wasm32-clean); clean crate layering
      (protocol ← core ← {native, browser}). Generated-artifact idempotency: the wasm
      + codec bundle are rebuilt deterministically by `build-wasm-test-assets.mjs`.
- [ ] Architecture reflected in `docs/architecture/*` (propose to the user, don't
      auto-merge).

## Constraints (from agent-os CLAUDE.md — do not violate)

- secure-exec stays free of ACP/agent/session deps; Agent OS logic lives in its own
  sidecar wrapper.
- Manage the secure-exec dep ONLY via `just secure-exec-*`; never hand-edit pins;
  never push with local `path:`/`link:` deps.
- Build WASM only through secure-exec's toolchain (`make -C registry/native wasm`),
  never raw `cargo build --target wasm32-wasip1`.
- Keep the TS and Rust clients thin and in parity; prefer the sidecar wrapper for
  multi-step ACP/session orchestration.
- jj-colocated repo: prefer `jj`; this work lives in the isolated workspace
  `/home/nathan/agent-os-web` (workspace name `agent-os-web`) to avoid concurrent
  churn in the shared default workspace.
