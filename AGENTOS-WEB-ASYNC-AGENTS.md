# Agent OS Web — Async Agent Executor (kernel-in-worker) + pi on Chrome local inference

Status: **v2** (revised after a 4-lens adversarial subagent review: architecture/
correctness, browser-platform feasibility, tests/deliverability, security). Builds
on `AGENTOS-WEB-CONVERGENCE.md`. Cross-repo (touches `@secure-exec/browser` in
convwasi + `agentos-sidecar-*`); cross-repo work is authorized.

> Review verdict: the approach is **viable**, but v1 had three blocking holes that
> v2 fixes and makes normative: (1) an async agent cannot block on SAB for stdin
> while awaiting the LLM — stdio must be *split* (§3.2); (2) `now_ms`/timeouts and
> per-execution output routing are NOT "unchanged AcpCore" (§3.4); (3) the untrusted
> executor becomes a direct shared-memory frame producer, so identity must be
> channel-bound and frames validated as hostile (§7). Deliverability: pi-in-a-worker
> (an 11 MB Node CLI that `spawn`s itself) is the biggest risk and gets its own
> milestones + a mock-provider CI gate (§6, §8, §10).

---

## 1. Goals / non-goals

**Goals**
- **G1 — Async agents in-browser.** A real ASYNC ACP agent (awaits async work AND
  makes its own kernel syscalls) with full `create_session` + `session/prompt` in
  real Chrome.
- **G2 — pi on local inference.** pi runs in the browser using Chrome built-in AI
  (`LanguageModel` / on-device Gemini Nano) as its model backend. No remote egress.
- **G3 — e2e tests green.** Headless CI gates that genuinely prove the async
  executor and the pi-bundle path, plus a best-effort real-Nano smoke; native and
  the existing 7 converged Chromium tests stay green.
- **G4 — Working demo + verify.** A demo (pi answering a prompt on-device) verified
  by the existing **`agent-browser`** CLI driving real Chrome.

**Non-goals:** remote-LLM egress; high-concurrency multi-agent scheduling (single
in-flight is the shipped model); any change to native behavior.

## 2. The constraint

- A page's **main thread may not block**: `Atomics.wait()` throws there; only Worker
  threads may block (`[[CanBlock]]`). `Atomics.waitAsync` is non-blocking.
- The converged guest model already exploits this: the **guest worker blocks** on a
  syscall SAB; the kernel **services without blocking**. (Proven in-tree:
  `sync-bridge.ts` + `runtime-driver.ts`.)
- `create_session` inverts it: `AcpCore` is the orchestrator that must *send* to the
  agent and *block-wait* for its reply. To block-wait legally, `AcpCore` must run in
  a **Worker**, not the main thread.

## 3. Architecture

### 3.1 Topology — `N+1`, main thread spawns workers

```
main thread (relay, trusted)
  • spawns the kernel worker AND every execution worker (PRIMARY — see note)
  • mints + transfers each execution's SAB channel pair (kernel-authority, §7)
  • async postMessage relay for client calls (createVm/exec/ACP)
  • hosts main-thread-only host capabilities + the Prompt-API proxy (§5, §6)
        │ postMessage (async)            │ transfers a bound SAB pair (structured-clone arg)
  kernel worker (TCB)  ───SAB───┬───SAB───┬─── exec worker 1..N (UNTRUSTED: guest OR agent)
  • wasm kernel + AcpCore       │         │     • @secure-exec/browser worker runtime
  • single reactor (§3.3)       │         │     • blocks on SAB only inside sync syscall shims
  • Atomics.wait legal here     │         │     • stdin/LLM via event loop (§3.2)
```

- **Workers are spawned by the main thread**, not as nested workers. (Review: nested
  module-workers floor at Safari 16.4 + DevTools blindness; *and* a nested
  `new Worker()` can't finish loading mid-`pushFrame` while the kernel worker is
  blocked. The kernel worker is the channel **authority** but not the **spawner**.)
- **The wasm kernel lives only in the kernel worker.** Execution workers carry no
  kernel copy. Guest and agent are both just "an execution in its own worker."
- `N` executions ⇒ `N+1` workers (one shared kernel worker).

### 3.2 The two-channel stdio split (THE crux — review F8)

An async agent worker **cannot** sit blocked in `Atomics.wait` for stdin while also
running its event loop to `await` the LLM. So an execution worker uses **two
different mechanisms**, by direction and purpose:

| Direction / purpose | Mechanism | Why |
|---|---|---|
| agent **stdout/stderr/exit** → kernel | **SAB up-channel** + `Atomics.notify` | the kernel reactor is *blocked* in `Atomics.wait`; only a SAB notify can wake it |
| agent **syscall request** → kernel | **SAB up-channel** | same: must wake the blocked reactor |
| kernel → agent **syscall result** | **SAB down-channel**, agent blocks on it | the agent is *inside a synchronous syscall shim* (e.g. `fs.readFileSync`), legitimately blocked — exactly the existing guest model |
| kernel → agent **stdin** | **`postMessage`** to the agent worker | the agent is running its event loop (awaiting the LLM / next turn), NOT blocked; `postMessage` is event-loop-friendly. The kernel can `postMessage` even while it is about to block (send is synchronous/fire-and-forget) |
| agent ↔ main **LLM inference** | **`postMessage`** to the main-thread proxy | async; runs on the agent's event loop, never blocks the kernel (§6) |

So: **the execution worker blocks on SAB ONLY inside synchronous syscall shims**
(bounded, kernel-serviced promptly), and uses the **event loop** for stdin + LLM. Its
**stdout** is a non-blocking SAB write + notify. This is internally consistent and
mirrors the proven guest model; v1's "agent blocks on the down-channel for stdin"
was wrong.

### 3.2.1 IMPLEMENTATION UPDATE — re-entrancy forces a RESUMABLE AcpCore (supersedes §3.3's "AcpCore stays synchronous")

Discovered during M3 implementation: the sidecar's `pushFrame(&mut self)` takes an
exclusive wasm-bindgen borrow that **panics on re-entry**, and guest syscalls are
serviced *through* `pushFrame`. So a synchronous `AcpCore` that block-waits inside
`pushFrame` cannot service an agent's mid-turn syscall (it would need a second,
nested `pushFrame`). That includes pi, whose inference is a `net` syscall during
`session/prompt`. (Native dodges this because it is async/tokio: it `.await`-yields
and the agent's syscalls run on other concurrent tasks behind `tokio::Mutex`, not a
single synchronous call stack. The browser worker is single-threaded with one
`&mut` wasm borrow, so it has no equivalent — it must hand-roll the yield.)

**Resolution (implemented):** the browser `AcpCore` path is **resumable / event-fed**
— it RETURNS from `pushFrame` at each await point (releasing the borrow), and the
kernel worker resumes it via a fresh, non-nested `pushFrame` when agent output
arrives. The result is delivered as a deferred event, not the immediate `pushFrame`
return.
- `agentos-sidecar-core`: `begin_create_session` (spawn + write initialize, return)
  + `feed_agent_output` (advance the initialize→session/new→Created state machine;
  never blocks — its mock's `poll_output` is `unreachable!()`). DONE + unit-tested.
- `session/prompt` gets the same begin/feed treatment (TODO — pi needs it).
- **Native is untouched** (keeps the synchronous, tokio-concurrent path). Only the
  browser wire-driver + host bridge feed the resumable engine.

§3.3's reactor below is unchanged for SYSCALL/STDOUT routing; what changes is that
`poll_output` no longer block-waits inside one `pushFrame` — the kernel worker feeds
agent stdout into `feed_agent_output` across separate `pushFrame` calls, and
services the agent's syscalls in between (now legal — not nested).

### 3.3 The single reactor (corrected)

The kernel worker runs one reactor that `Atomics.wait`s on a single global `GEN`
signal any execution bumps, and on wake drains every up-channel:

```
// kernel worker reactor (drives AcpCore's poll_output; see §3.4)
loop {
  drainedSomething = false
  for each live execution e {
    while (frame = e.up.tryRead()) {            // single-producer ring, validated (§7)
      drainedSomething = true
      switch frame.kind {
        SYSCALL: r = kernel.service(e.id, frame); e.down.writeResult(r); e.down.notify()
        STDOUT|STDERR: e.outQueue.push(frame)   // PER-EXECUTION queue, never VM-wide
        EXIT:   e.exited = true; e.outQueue.push(EXIT)
      }
    }
  }
  if (awaited execution has a queued frame) return it to AcpCore   // §3.4
  gen = Atomics.load(GEN)                        // SNAPSHOT AFTER the drain
  if (!drainedSomething) Atomics.wait(GEN, gen, remainingBudgetMs)  // timed (§3.4)
}
```

Corrections folded in:
- **Per-execution output queues (F3).** v1 reused the VM-wide
  `poll_execution_event` + filter, which *drops/cannibalizes* another execution's
  stdout. The reactor drains each up-channel into the **owning execution's** queue;
  `poll_output(e)` dequeues only from `e`'s queue.
- **GEN snapshot AFTER drain, wait only if nothing drained (F4).** Prevents both the
  lost-wakeup and the busy-spin (loading GEN at the top of the loop spins).
- **Memory ordering (F5).** `GEN` is the **last** release store, *after* the ring
  tail/bytes; the consumer acquire-loads `GEN` before reading the ring. The ring
  **tail** (atomic) publishes the frame; `GEN` is purely the wakeup. (Mirrors
  `runtime-driver.ts`'s data→LENGTH→STATE_READY→notify order.)
- **The kernel NEVER blocks on an untrusted worker (F7, security).** It only
  `Atomics.wait`s on `GEN`. Down-channel writes (syscall results, stdin-by-SAB if
  ever) must never block on a full untrusted ring; size/credit them so they cannot
  fill (§4). Only the *untrusted* up-channel producer may block.

### 3.4 What actually changes in AcpCore (v1's "unchanged" was false)

`AcpCore`'s Rust logic (initialize → session/new → prompt) is reused, but the
browser `AcpHost` impl must change in three ways:

1. **Real clock, not a poll counter (F1, BLOCKING).** Today `now_ms()` returns a
   poll counter and `send_json_rpc` tight-spins. Once `poll_output` blocks on
   `Atomics.wait`, that model breaks: a hung agent (no GEN bumps) would freeze the
   TCB forever and no timeout fires. Fix: `now_ms()` returns a **real monotonic
   clock** in the kernel worker (`performance.now()` over the wasm boundary), and the
   blocking `poll_output` uses the **timed** `Atomics.wait(GEN, gen, remainingMs)`
   with `remainingMs = deadline - now_ms()`. The host must carry the per-call
   deadline. Engine timeouts (`INITIALIZE_TIMEOUT_MS=10_000`, prompt `600_000`)
   regain their wall-clock meaning.
2. **Per-execution `poll_output` (F3).** Map `poll_output(process_id)` to that
   execution's queue (§3.3), not the VM-wide event stream.
3. **`write_stdin` posts (not SAB).** `write_stdin` delivers via `postMessage` to the
   async agent (§3.2), then `poll_output` blocks for the SAB-delivered stdout.

The *native* `AcpHost` is untouched; only the browser impl diverges (and the
host-free `AcpCore` engine itself stays shared — do NOT fork it).

### 3.5 Concurrency: single-in-flight, and the cross-execution-syscall limit

- **Shipped model:** the main-thread relay **serializes ACP requests** so only one
  orchestration is ever in-flight in the shared kernel worker (review F9 — the
  simplest correct option). This makes the head-of-line concern (§v1 3.4) a non-issue
  for the single-agent demo *by construction*.
- **Hard limit to state (F2, BLOCKING-as-a-claim):** the single synchronous reactor
  is deadlock-free **only for self-contained agent syscalls** (a syscall the kernel
  answers without another execution making progress). If execution X's syscall result
  requires another execution Y to run — which would need a *second* `AcpCore`
  orchestration the single synchronous call stack cannot enter — it deadlocks. So:
  **invariant — under shared-worker mode, agent kernel syscalls must be self-contained
  (no blocking on another execution's progress).** Inter-agent dependencies are
  unsupported here.
- **Future concurrency** (out of scope; pick later): (i) one kernel worker per
  session/VM (no HoL, ~2 workers/agent, multiple sidecars); (ii) async/resumable
  `AcpCore` (one shared worker, but this **forks the shared core** — violates the
  native/browser-share-one-core constraint, real cost); (iii) a bounded kernel-worker
  pool.

## 4. SAB protocol (normative)

Per execution: a **duplex** pair of SAB ring buffers (`up`: execution→kernel; `down`:
kernel→execution) plus one shared global control SAB.

- **One physically distinct SAB pair per execution (F2/security).** TCB-allocated,
  never aliased into another execution's view, never reused without an **epoch**
  counter. NO VM-wide data SAB shared across guests.
- **Single producer per ring (F6).** Exactly one writer per up-channel (the execution
  worker's main turn-loop); STDOUT/STDERR/SYSCALL/EXIT are serialized through that one
  writer. No second context writes the same ring; if async stdout/proxy paths exist
  they funnel through that single writer (else CAS the tail).
- **Publish order (F5):** `write bytes → Atomics.store(tail) → Atomics.store(GEN,
  gen+1) → Atomics.notify(GEN)`. Consumer: `Atomics.load(GEN) → Atomics.load(tail) →
  read bytes`. The tail publishes; GEN wakes.
- **No lost wakeup / no busy-spin (F4):** reactor snapshots `GEN` *after* a full
  empty-drain, waits only if nothing drained.
- **Backpressure asymmetry (F7, hard):** the **untrusted up-channel producer may
  block** when full; the **TCB must never block** on a full down-channel — size the
  down-channel to max in-flight (one syscall result + bounded control) or use
  credits, and if absent, park that execution's orchestration and keep the reactor
  running. A malicious worker that never drains must not wedge the kernel.
- **Validation (F3, security):** copy-then-validate-then-parse. Snapshot `len` + body
  into kernel-private memory in one bounded read; validate `0 ≤ len ≤
  BROWSER_MAX_FRAME_BYTES` and `len ≤ ring_capacity` against the snapshot only (never
  re-read SAB after the check → no TOCTOU); range-check head/tail every step. Any
  violation → kill that execution + free its channels (epoch-bump), never a hang.
- **Per-execution fairness:** cap frames/bytes drained per execution per wake so one
  execution can't starve the shared reactor.
- **Teardown (F10/F7):** invalidate the `channel→identity` binding (epoch) *before*
  freeing; write a poison/EOF sentinel to the execution's down-channel and notify it
  so a parked syscall shim wakes; the worker checks the poison after every wake. A
  killed execution's channels are frozen; a zombie worker cannot be re-driven.

## 5. Execution worker runtime

- Reuse the `@secure-exec/browser` worker for guest syscalls (the SAB blocking shim
  already exists for guests).
- For an **agent execution**, bridge the agent's stdio per §3.2: stdout/exit → SAB
  up-channel; stdin ← `postMessage`; syscalls → SAB shim (blocking).
- **Host-capability callback proxy (platform review, MAJOR).** Today the main thread
  services `child_process.*`, the network adapter, `process.signal_state`, `onStdio`,
  and `onFsReadDenied`. When the kernel moves into a worker these can't follow as
  host closures; the kernel worker must **proxy them back to the main relay via async
  RPC** (they are already async, so awaiting them off the blocking path is fine). M2
  must build this or the converged `child_process`/network tests fail.
- **SABs are passed as structured-clone arguments, never in a transfer list** (SABs
  aren't transferable).

## 6. pi + Chrome local inference (RESOLVED design)

Two decisions are locked. **D3 — mirror native exactly:** native pi reaches its
model over HTTP — it reads a provider `baseUrl` (`~/.pi/agent/models.json` /
`*_BASE_URL`) pointing at a host server (the e2e uses `llmock`,
`crates/client/tests/helpers/llmock-server.mjs`). The browser runs the **same** pi
adapter (`@agentos-software/pi/dist/adapter.js`) the native sidecar runs, and only
swaps where `baseUrl` points. **D1 — inference is a kernel-brokered HTTP endpoint,
not a special binding:** Chrome's on-device model is exposed as a local OpenAI/
Anthropic-compatible endpoint that pi reaches over the kernel's loopback socket
table, so it is mediated by construction (it is just kernel-brokered traffic the
policy can gate) and pi needs **zero** changes beyond `baseUrl`.

**Chosen implementation — in-sandbox proxy + existing host-callback (no new kernel
plumbing):**

- **The "LLM proxy" is the browser twin of `llmock-server.mjs`.** Same role (a
  trusted-ish HTTP service pi talks to), different backend (Chrome `LanguageModel`
  instead of a mock/real API).
- **It runs as a normal guest execution.** A tiny OpenAI/Anthropic-compatible HTTP
  proxy server runs as a guest in the sandbox, listening on a loopback port (ordinary
  guest networking — loopback already works). pi connects to it over loopback. pi is
  pointed at that loopback `baseUrl` exactly as native pi is pointed at `llmock`.
- **It reaches Chrome via the EXISTING host-callback mechanism — no kernel changes.**
  secure-exec already has a generic, kernel-brokered guest→host callback path
  (`sidecar-core/tools.rs` + `router.rs`; agent-os registers kits/tools via
  `RegisterHostCallbacks`, guest invokes, kernel routes to a host handler over the
  `host_callback` wire callback). The proxy guest invokes a registered **`chrome-llm`
  host callback** instead of inventing a new syscall. This keeps **secure-exec
  untouched** (repo-boundary win) and is **mediated for free** (host callbacks are
  kernel-brokered + policy-gated → resolves D1 with no special capability plumbing).
- **The host-callback handler is the only trusted, main-thread piece.** Registered by
  agent-os, running on the main-thread relay (where `LanguageModel` reliably lives —
  worker exposure is undependable), it calls `LanguageModel.availability()/create()/
  prompt()` and returns the completion. The global is **`LanguageModel`** (not
  `window.ai`/`self.ai`).
- **Non-streaming for now (keep it simple).** The proxy does request→response
  completions only (`session.prompt()`, not `promptStreaming`). The host-callback
  mechanism's request/response shape is sufficient; SSE/streaming is a deferred
  follow-up. pi must run against a non-streaming OpenAI/Anthropic completion.
- **pi launch — mirror native (D3).** Load the same in-process
  `@agentos-software/pi/dist/adapter.js` the native e2e loads; if that adapter itself
  `child_process.spawn`s pi, the browser must honor that spawn through the kernel
  process table exactly as native does (whatever native does, the browser mirrors —
  this is the convergence thesis, not a new fork). No pi/pi-ai fork; pi just gets a
  `baseUrl`.
- **Gemini Nano budget (platform review).** Nano's context is small
  (`session.inputQuota`/`inputUsage`). pi's default system prompt + tools overflow it;
  the demo needs a **trimmed pi profile** (minimal/no tools, no file context) and a
  tiny task. Add a "fits-Nano" check.
- **Trust:** model output is untrusted bytes in both directions (size caps; the
  host-callback handler exposes only a bare completion — no `create()` options that
  enable tools/function-calling/system-injection, no live session handles returned to
  the guest). The proxy guest is itself untrusted; it is just an OpenAI↔host-callback
  translator with no extra privilege.

Net: the only NEW trusted code is the **main-thread `chrome-llm` host-callback
handler** (a small `LanguageModel` adapter); everything else is an untrusted guest
proxy + pi's stock HTTP client + the existing host-callback plumbing.

## 7. Security invariants (normative — review: §9 was asserted, not engineered)

1. **Channel-derived identity, never frame-asserted.** The kernel derives `(vm_id,
   execution_id)` solely from *which SAB channel* a frame arrived on; any id bytes in
   an executor-written frame are ignored. (Else execution A spoofs B / another VM.)
2. **Kernel memory is never shared.** The kernel `WebAssembly.Memory`/module is never
   `postMessage`'d/transferred to any execution worker. Making `Atomics.wait` legal in
   the kernel worker must NOT be done by giving the kernel wasm-shared-memory that then
   leaks the heap. The control + channel SABs are distinct allocations from kernel
   memory.
3. **One physically distinct, TCB-allocated SAB pair per execution; epoch on reuse.**
   No VM-wide shared data SAB; channel-id reuse carries a generation to prevent
   stale-frame confusion.
4. **Copy-then-validate-then-parse hostile frames** (§4): bounds + ring-index checks
   as hostile input; no post-check re-read.
5. **Bounded work/bytes per execution per pass; validation failure kills that
   execution, never hangs the kernel worker.** DoS-resistance is a boundary property
   (one bad agent must not wedge the single TCB worker that serves every VM).
6. **The kernel worker is the sole authority** for channel allocation and the
   `channel→identity` binding, established **before** executor code runs, regardless
   of who spawns the worker. The main thread (if it spawns) is a dumb conduit that
   transfers an already-bound pair.
7. **Inference is reached as kernel-brokered traffic, never an ambient host binding
   (D1, RESOLVED).** The model is exposed as a loopback HTTP endpoint backed by the
   existing kernel-brokered host-callback mechanism; the guest reaches it as ordinary
   policy-gated traffic, so "kernel is the sole enforcement point" holds after the
   thread move. No new guest-visible host binding is added. The trusted `chrome-llm`
   host-callback handler exposes only a bare completion (no tool/function-calling/
   system-injection `create()` options, no live session handles returned).
8. **The kernel/TCB never blocks on an untrusted worker** (down-channel full, or a
   worker that won't drain). Only the untrusted up-channel producer may block.

## 8. Test plan

- **Unit (vitest), logic:** reactor state machine + ring framing against fake
  executions — per-execution routing, backpressure asymmetry, exit, teardown/epoch,
  validation rejects. Deterministic, no real workers.
- **Real-worker race stress (Chromium, F6):** a 2-execution Playwright test that
  hammers the GEN/notify ordering with real `Atomics` over many iterations — the lost-
  wakeup/memory-ordering requirement is unverifiable single-threaded.
- **Headless CI gate 1 — hardened async-echo (F1).** The async echo agent's
  `session/prompt` reply must **causally depend on a kernel `fs.readFile` it `await`s
  *during* AcpCore's `poll_output`**: it reads a file and echoes those exact bytes
  back in the response; the test asserts the bytes. AND a **second** execution's
  syscall is serviced while AcpCore is blocked on the first. This proves
  inline-syscall-during-block-wait + "services everyone" — not a `setTimeout` that
  passes while the real path deadlocks.
- **Headless CI gate 2 — pi + MOCK `chrome-llm` host-callback.** pi boots in the
  worker, connects over loopback to the in-sandbox OpenAI/Anthropic proxy, and runs
  `create_session` + `session/prompt`; the registered `chrome-llm` host callback is a
  **mock** that returns a fixed sentinel (non-streaming; mirrors the native
  `llmock`/`PONG_FROM_LLMOCK` precedent). Exact-sentinel assertion, no Nano dependency
  → runs on any CI runner. This is the real gate for the heavy pi-bundle +
  proxy-guest + host-callback path.
- **Best-effort smoke — pi + real Nano (non-gating).** Swap the mock handler for the
  real `LanguageModel.prompt()` (non-streaming); assert a **loose structural**
  predicate only (non-empty, bounded length, produced within timeout,
  `availability()==="available"`); **never assert exact Nano text**. Skipped when the
  model is unavailable.
- **Regression:** the existing 7 converged Chromium tests (now through the kernel
  worker) + 6 vitest + native cargo stay green; secure-exec's own converged tests
  stay green (cross-repo lockstep).

## 9. Demo + verify CLI (`agent-browser` — confirmed existing)

- **`agent-browser` exists** (`/home/linuxbrew/.linuxbrew/bin/agent-browser`): a CDP
  CLI with `open/eval/get text/console/errors/wait`, `--args`, `--executable-path`,
  `--headed`. §v1 "OPEN: existing vs build" → **existing; reuse it.**
- **Verify = `agent-browser` + the COOP/COEP server** (reuse
  `tests/browser-wasm/serve.mjs`, which already sets cross-origin isolation —
  `agent-browser` drives a page, it can't host one). Prompt-API flags are
  `chrome://flags`-gated, not `--switch`-toggleable, so the smoke check needs a
  **prepared Chrome** (`--executable-path` + a persistent `--user-data-dir` profile
  with the feature enabled and Nano provisioned).
- **Two-tier assertion:** the deterministic gate uses the **mock `chrome-llm`
  host-callback** (exact sentinel via `agent-browser eval`/`get text`, `errors`
  asserts no exceptions); the real-Nano tier is the loose, best-effort smoke (§8).
- **Demo page:** a trimmed pi session; user prompt → on-device answer (non-streaming
  for now); surfaces "model: Chrome built-in (on-device)".

## 10. Milestones

Implementation status (2026-06): M1, M2a, M3, M4 (mechanism), M4d, M5 **DONE + green in
Chromium**. Per a scope decision, the M4 "agent" was delivered as a leaner ACP agent
(answering via the proxy + Chrome inference) rather than the literal pi binary; booting
pi itself (M4a) is the remaining stretch follow-on. Gates live under
`packages/browser/tests/browser-wasm/async-*.spec.ts` + `scripts/verify-demo.mjs`.

- **M1 — Reactor + SAB. ✅ DONE.** `sab-ring`/`sab-reactor`/`sab-execution-endpoint` in
  `@secure-exec/browser`; duplex rings + validation; real-worker GEN/notify race stress
  green. 26 vitest.
- **M2a — Kernel boots in a worker. ✅ DONE.** wasm sidecar in a dedicated worker; main
  thread = relay; main-thread agent spawn (review F11). 
- **M2b — Converged guests through the kernel worker.** Subsumed by M3 for the agent
  path (fs.* + net.* syscalls route through the in-worker kernel via the reactor's
  `serviceSyscall` → pushFrame). A broader sweep of all legacy converged guest tests
  through the worker kernel remains open.
- **M3 — Async-agent executor. ✅ DONE.** Resumable `AcpCore` (§3.2.1); agent in an
  execution worker; stdio split (§3.2); GuestRequest servicing via the reactor;
  hardened async-echo (mid-turn fs syscall) CI gate green in Chromium.
- **M4 mechanism — async-inference transport + in-sandbox proxy. ✅ DONE.** DEFERRED
  host-callback syscall + reactor completion channel (the one async hop); the
  `chrome-llm` adapter (mock + real `LanguageModel`); loopback TCP through the executor;
  the in-sandbox OpenAI proxy (HTTP-over-loopback ↔ `host.inference`). Gates:
  `async-infer`, `async-loopback`, `async-proxy` — all green in Chromium; +`openai-proxy`
  framing vitest.
- **M4a — pi boots headless. ✅ DONE.** The real `@agentos-software/pi/dist/adapter.js`
  (the same one the native sidecar launches), esbuild-bundled CJS, boots as a guest in
  the browser converged node-stdlib executor in Chromium and answers the ACP
  `initialize` handshake (`agentInfo.name = pi-sdk-acp`). Gate: `pi-boot.spec.ts`.
  Required two convwasi executor additions: `node:stream` + `node:module` guest
  polyfills (pi's builtins; fs/path kernel-backed), and the persistent-execution mode
  (`ExecOptions.persistent`) that keeps the worker event loop alive for pi's async
  WHATWG-stream stdin pump so the reply flushes before exit.
- **M4b — pi answers session/prompt via the model. ✅ DONE.** The real full pi SDK
  (adapter + pi-coding-agent + pi-agent-core + pi-ai, a 16 MB self-contained `.cjs`)
  completes an ENTIRE ACP turn in the browser converged executor in Chromium:
  `initialize → session/new → session/prompt → a model answer`. pi-ai's global `fetch`
  reaches the model endpoint, parses the Anthropic SSE, and returns the assistant
  message. Gate: `pi-prompt.spec.ts`. The endpoint is a mock chrome-llm SSE server
  (`serve.mjs /v1/messages`); the in-sandbox proxy guest + host-callback is the
  production refinement (mechanism proven by `async-proxy`). Required convwasi fixes:
  the full node-builtin batch + dynamic-`import()` routing + guest global `fetch` + two
  real bugs (`process.stdout.write` ignored its completion callback → the ACP writer
  blocked after the first response; the guest fetch recursed into itself via the network
  adapter). pi is statically bundled (`__piSdkModules` preload — no node_modules mount).
- **M4c — Real Nano behind the same host callback.** Wired + ready: the demo already
  probes `createChromeLanguageModelSession()` and uses real `LanguageModel.prompt()`
  when present (`AGENT_BROWSER_EXECUTABLE_PATH` → a Chrome with Nano); best-effort tier.
- **M4d — Demo page. ✅ DONE.** `agent-demo.html` + `agent-demo.entry.ts`
  (`window.__agentDemo.run(prompt)`), real-`LanguageModel`-or-offline-mock.
- **M5 — `agent-browser` verify. ✅ DONE.** `scripts/verify-demo.mjs` + `serve.mjs`
  drive the demo in a real browser via the `agent-browser` CLI (open → wait ready →
  eval → assert; PASS/FAIL exit). offline-mock tier gates deterministically; Nano tier
  best-effort.

## 11. Decisions (RESOLVED)

- **D1 — Prompt API access: RESOLVED → kernel-brokered HTTP endpoint.** Inference is
  an in-sandbox OpenAI/Anthropic proxy reaching Chrome via the existing host-callback
  mechanism, so it is mediated by construction (no ambient binding, no new kernel
  plumbing). §6.
- **D2 — Concurrency: RESOLVED → single-in-flight + shared kernel worker** (with the
  self-contained-syscall limit, §3.5); multi-agent deferred.
- **D3 — pi launch: RESOLVED → mirror native exactly.** Run the same
  `@agentos-software/pi/dist/adapter.js` the native sidecar runs; the browser honors
  whatever native does (incl. any `child_process.spawn`) through the kernel — no fork.
  pi only gets a different `baseUrl`. §6.
- **D4 — CI scope for Nano: RESOLVED → real-Nano is manual/best-effort**; the
  mock-`chrome-llm` gate (M4b) is the headless CI proof. §8.

## 12. Top residual risks

1. **pi-in-a-worker (M4a)** — an 11 MB Node CLI that `spawn`s itself; biggest
   deliverability risk; the convergence memory already flagged it. Mitigated by the
   in-process-adapter path (D3) + the M4a gate before any inference.
2. **M2b re-architecture** — inverting the converged servicing direction across two
   repos in lockstep without regressing secure-exec's tests.
3. **Nano capability** — context too small for any non-trivial pi task; the demo task
   must be chosen to fit, and real-Nano is best-effort only.
4. **The self-contained-syscall limit (§3.5)** — fine for the demo; revisit before
   any multi-agent or inter-agent-dependency use.
