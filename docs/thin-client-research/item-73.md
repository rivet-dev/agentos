# Item 73 research: execute browser ACP adapters from the projected VFS

Status: implementation-ready research only. This note does not modify
production code, tests, generated assets, or the Item 73 tracker status.

## Recommendation

Replace the browser ACP factory's optional synchronous fake with an asynchronous
host-routing seam backed by the standard `@rivet-dev/agentos-runtime-browser`
Worker. Keep the wasm sidecar and the authoritative projected VFS on the main
thread. The runtime host should bind the driver's already-running primary Worker
when Rust registers an ordinary `exec`/`run` execution, but start one additional
ordinary production Worker for each sidecar-requested ACP adapter execution.
That distinction prevents the existing primary execution from being launched
twice. Each ACP Worker executes the exact entrypoint path supplied by the Rust
sidecar and asynchronously forwards only stdin/stdout/stderr/exit and guest
syscall traffic.

Preserve two distinct frame paths:

```text
guest syscall in a Worker
  -> synchronous raw wasm pushFrame
  -> sidecar/kernel response

public ACP list/create/prompt
  -> asynchronous pushFrameAsync
  -> raw wasm pushFrame returns AcpPending internally
  -> await exact adapter Worker's output
  -> raw deliver/abort frames back to the sidecar
  -> final ACP response only
```

Do **not** make the raw guest-syscall dispatcher asynchronous. A Worker blocked
in the existing SharedArrayBuffer sync bridge requires that path to remain
synchronous. Add a second, explicitly asynchronous frame method for extension
requests instead of using a `Uint8Array | Promise<Uint8Array>` union.

Delete `AgentOsConvergedSidecarOptions.agentExecutor`, `SyncAgent`,
`SyncAgentExecutor`, the in-process line handler/session maps, and the
path-to-prebuilt-worker lookup in the browser-WASM fixture. The only caller-side
data remains opaque package bytes and explicit runtime configuration. Package
metadata, agent resolution, entrypoint selection, ACP state, deadlines, restart
decisions, and cleanup stay in Rust.

Priority: **P1**. Boundary confidence: **high**. Implementation confidence:
**medium-high**. The standard Worker, synchronous kernel bridge, projected VFS,
resumable Rust ACP core, and package projection all exist. The nontrivial part is
extracting the existing Worker controller so multiple isolated adapter Workers
can share one sidecar VM without sharing mutable Worker globals.

## Original issue

The tracker entries are at `docs/thin-client-migration.md:119,201,288`.

The public factory currently advertises browser ACP support but defaults to a
bridge that never executes an adapter:

- `createAgentOsConvergedSidecar` constructs
  `createConvergedExecutionHostBridge({ agentExecutor: undefined })`;
- `startExecution` merely echoes `nextExecutionId`;
- `createWorker` merely returns a generated string;
- stdin, kill, event polling, and worker termination have no real execution to
  control; and
- a pending ACP create/prompt therefore cannot produce adapter output.

The optional workaround is not production behavior. It accepts a
`SyncAgentExecutor`, parses newline-delimited ACP JSON in TypeScript, retains
sessions and output queues, and creates fake output in the browser client. That
is exactly the parallel client implementation Item 73 must remove.

The browser-WASM async tests prove that a Worker/reactor arrangement can run, but
they do not prove the public factory. `async-kernel.worker.ts` maps an argv path
through `AGENT_WORKERS` and falls back to `DEFAULT_AGENT_WORKER_URL`; the packed
fixture entrypoint itself is only this comment:

```js
// Browser worker execution is supplied by the test host bridge.
```

Changing or deleting the projected file therefore does not change which test
program executes. The test proves a fixture lookup table, not Linux-like
execution of `/opt/agentos/bin/<adapter>` from the VM filesystem.

## Exact current flow and ownership

| Layer | Current symbol | Current behavior | Item 73 disposition |
| --- | --- | --- | --- |
| Public AgentOS browser factory | `packages/browser/src/converged-sidecar.ts:72-100,116-252` | Exposes `agentExecutor`; wraps raw wasm `pushFrame` with a synchronous pending driver. | Remove the fake option. Return raw synchronous `pushFrame` plus asynchronous `pushFrameAsync`, both using the same wasm instance. |
| AgentOS JS host bridge | `packages/browser/src/converged-execution-host-bridge.ts:24-288` | Has driver/no-op and synchronous-agent modes; stores fake sessions, output arrays, `lastAgentExecutionId`, and `pendingProcesses`. | Replace with a thin JSON-to-typed runtime-host adapter. It must distinguish binding the reserved primary Worker from spawning an ACP service Worker; no agent implementation or output policy belongs here. |
| Pending router | `packages/browser/src/acp-pending-driver.ts:7-221` | `pollAgentOutput` and the returned frame function are synchronous; binds a pending process to the most recently spawned fake execution. | Make the router asynchronous and consume a sidecar-returned exact `{ processId, executionId }` route. Fold/delete the extra synchronous drive-loop layer if that keeps the diff smaller. |
| Runtime factory contract | `packages/runtime-browser/src/runtime-driver.ts:68-95` | `loadSidecar()` receives no execution host; handle exposes only synchronous `pushFrame`. | Pass a runtime-owned execution host to `loadSidecar(host)`. Add optional `pushFrameAsync` on the returned handle. |
| Production Worker owner | `packages/runtime-browser/src/runtime-driver.ts:451-554` | Owns one standard Worker, its sync bridge, request correlation, stdio routing, and converged servicer. | Extract the reusable per-Worker controller. Bind that controller for ordinary execution registration; create another controller only for an unbound, sidecar-requested adapter execution. |
| Public extension request | `packages/runtime-browser/src/runtime-driver.ts:1054-1064` | Posts `type: "extension"` to the guest Worker. | Route through the main-thread converged session and `pushFrameAsync`; the guest Worker intentionally rejects extension dispatch. |
| Worker execution protocol | `packages/runtime-browser/src/worker-protocol.ts:57-157`; `worker.ts:3472-3730` | Runs caller-supplied source strings. `persistent` has a client-owned 120-second timeout. | Add a sidecar-service request that executes a VFS entrypoint path, remains alive until sidecar kill/EOF, and does not use the generic persistent timeout. |
| Projected module loader | `packages/runtime-browser/src/worker.ts:2041-2073`; `converged-module-servicer.ts` | Worker module resolution/load already calls the main-thread kernel-backed module servicer. | Reuse unchanged. The entrypoint must be resolved and loaded through this seam, never fetched/read by the main thread. |
| Sidecar execution bridge | `crates/native-sidecar-browser/src/service.rs:1895-2075` | Retains the context's `BrowserWorkerEntrypoint` and sends it in `BrowserWorkerSpawnRequest`; owns kernel PID and worker release. | Already the correct authority. Keep it; add no TypeScript entrypoint inference. |
| ACP launch adapter | `crates/agentos-sidecar-browser/src/acp_host.rs:101-190` | Resolves projected agent metadata, passes the exact adapter path into `CreateJavascriptContextRequest`, and retains process -> execution ownership. | Already correct. Expose that exact route to the pending driver rather than reconstructing it with "last execution" state. |
| Wasm wrapper | `crates/agentos-sidecar-browser/src/wasm.rs:14-181` | Exposes opaque pending process/timeout/frame helpers but not the corresponding execution ID. | Retain extension diagnostics and expose one opaque pending-route helper returning both IDs after ownership validation. |
| Browser test fixture | `packages/browser/tests/browser-wasm/async-kernel.worker.ts:27-41,120-154`; `packages/browser/scripts/build-wasm-test-assets.mjs` | Substitutes a prebuilt worker by path; generated package bin has no executable implementation. | Delete the lookup/default. Pack executable adapter source and run it through the public factory/standard Worker. |

The Rust sidecar is already the right authority for projected metadata and
execution lifetime. A Web Worker cannot be created by Rust/WASM without calling
the browser host, so Worker allocation and message correlation are legitimate
host-only state. The host must not choose the adapter, synthesize its source, or
implement ACP.

## Exact production edits

### 1. Split raw synchronous and pending-aware asynchronous frame dispatch

In `packages/runtime-browser/src/runtime-driver.ts`, change the contracts to the
following shape (names may vary, but keep the separation explicit):

```ts
export interface ConvergedSidecarHandle {
	/** Raw, non-awaiting wasm dispatch used by guest sync syscalls. */
	pushFrame(frame: Uint8Array): Uint8Array;
	/** Pending-aware dispatch used by public extension requests. */
	pushFrameAsync?(frame: Uint8Array): Promise<Uint8Array>;
	setNextExecutionId?(executionId: string): void;
}

export interface ConvergedSidecarFactoryOptions {
	loadSidecar(
		executionHost: ConvergedExecutionHost,
	): Promise<ConvergedSidecarHandle>;
	// existing config/packages/codec fields remain unchanged
}
```

Define `ConvergedExecutionHost` in runtime-browser, not in the AgentOS package.
It is the generic browser capability that owns standard Workers. It should
accept the already-decoded `createWorker` request, route byte-preserving stdin,
kill/terminate exact executions, and asynchronously wait for exact execution
output. Suggested minimum surface:

```ts
export interface ConvergedExecutionHost {
	bindExistingExecution(executionId: string): void;
	reserveExecution(request: {
		vmId: string;
		argv: readonly string[];
		env: Readonly<Record<string, string>>;
		cwd: string;
	}): { executionId: string };
	createWorker(request: ConvergedWorkerSpawnRequest): {
		workerId: string;
		runtime: "java_script" | "web_assembly";
	};
	writeStdin(executionId: string, chunk: Uint8Array): void;
	closeStdin(executionId: string): void;
	kill(executionId: string, signal: number): void;
	terminate(executionId: string, workerId: string): void;
	waitForOutput(
		executionId: string,
		deadlineMs: number,
		isCancelled?: () => boolean,
	): Promise<{ kind: "stdout" | "stderr" | "exit"; payload: Uint8Array } | null>;
}
```

The exact public names can differ, but the host needs two unambiguous routes:

1. **Pre-bound primary execution.** Before `ConvergedExecutorSession` registers
   the driver's ordinary guest execution, bind its exact execution ID to the
   existing primary `BrowserWorkerSession`. The following Rust
   `startExecution`/`createWorker` callbacks consume that reservation and attach
   the Rust worker ID to the same Worker. They must not allocate a second Worker.
2. **Sidecar-requested service execution.** An ACP launch has no primary
   reservation. Its `createWorker` callback creates an isolated standard Worker
   controller from the exact Rust spawn request and stores the execution/worker
   route for stdin, output, signals, and teardown.

This correlation is legitimate host-only state; it does not choose an agent or
implement runtime policy. A duplicate reservation, mismatched execution ID, or
second worker binding must fail with a typed error instead of falling back to a
new Worker.

`ConvergedWorkerSpawnRequest` must mirror the data Rust already serializes in
`native-sidecar-browser/src/wasm.rs:884-917`: VM, context, execution, runtime,
entrypoint, process identity/config, OS config, and WASM tier. Do not omit the
entrypoint and recover it from argv in TypeScript.

Update `packages/runtime-browser/src/default-sidecar.ts`, runtime tests' fake
sidecars, and browser harness loaders for the new `loadSidecar(host)` argument.
The generic sidecar should delegate its execution callbacks to the supplied host
instead of creating another no-op bridge.

### 2. Extract the existing standard Worker controller

Move the per-Worker mechanics currently embedded in
`BrowserRuntimeDriver`—`Worker`, control token, pending request map, sync bridge,
stdio routing, stdin/signal controls, and cleanup—into a package-private class,
for example `packages/runtime-browser/src/browser-worker-session.ts`.

`BrowserRuntimeDriver` should use one instance for ordinary `exec`/`run` and
pre-bind it as described above. The new converged execution host should create a
separate instance only for a `createWorker` callback that has no existing-worker
reservation (the ACP service case). This is necessary because `worker.ts` has
realm-global mutable state (`activeExecutionId`, module cache, process config,
stdin hooks, signals). Running two live ACP adapters in one Worker would corrupt
correlation, while blindly creating a Worker for the primary registration would
run ordinary guest code twice.

Each adapter Worker must share the already bootstrapped `ConvergedServicer`, not
construct another sidecar or VM. Give each Worker session its own sync-bridge
buffers, but route its `BrowserWorkerSyncRequestMessage` to:

```ts
convergedServicer.route(exactExecutionId, operation, args, hostCapabilityFallback)
```

Use the `executionId` from the Rust `BrowserWorkerSpawnRequest` as the only route
key. Validate `requestId + executionId + controlToken` exactly as the current
driver does. Do not create one `BrowserRuntimeDriver` per adapter: that would
bootstrap a different VM and the adapter would not see the ACP owner's projected
filesystem.

### 3. Add a standard Worker "execute projected entrypoint" mode

In `packages/runtime-browser/src/worker-protocol.ts`, add a distinct request,
for example:

```ts
| {
	controlToken: string;
	id: number;
	type: "execute-entrypoint";
	payload: {
		executionId: string;
		entrypoint: string;
		argv: string[];
		env: Record<string, string>;
		cwd: string;
	};
}
```

In `packages/runtime-browser/src/worker.ts`, execute that path through the
already-installed runtime `require`/module loader. The Worker must not receive
the entrypoint's source from the main thread. A fixed internal bootstrap such as
calling the runtime `require(entrypoint)` is fine; fetching, `readFile` on the
main thread, mapping a path to a bundle URL, or embedding a source fallback is
not.

Resolve `/opt/agentos/bin/*` through the existing VFS/module seam and accept the
normal executable JavaScript shebang in the loaded file (strip it in the
Worker-side module compiler only if the existing compiler does not already do
so). Initial Item 73 support may be JavaScript-only if that is the only native
ACP adapter runtime currently supported, but a Rust request for an unsupported
`web_assembly` entrypoint must fail with a typed unsupported-runtime error. It
must never substitute JavaScript source or a bundle URL.

This mode is a sidecar-owned service execution, not generic
`ExecOptions.persistent`. It must:

- set `process.argv`, cwd, env, pid/ppid/uid/gid from the Rust spawn request;
- keep streaming stdin open;
- report the real exit code;
- remain alive until the sidecar closes stdin, sends a signal, or terminates the
  worker; and
- **not** use `PERSISTENT_EXEC_TIMEOUT_MS` at `worker.ts:90-93,3548-3556`.

The ACP core already supplies phase deadlines and owns session lifetime. A
second 120-second Worker timeout would recreate client policy and kill healthy
long-lived sessions.

### 4. Preserve output and bound the unavoidable host queue

The current stdio path truncates each message through `boundStdioMessage` at
`worker.ts:1217-1222,1741-1748`. Truncating an ACP JSON-RPC line produces invalid
JSON. Change this path to chunk without data loss, or add a binary execution
output message used only by sidecar-managed service Workers.

The runtime-owned execution host may retain output while the asynchronous
pending driver is between turns, so that queue must be bounded. Use both:

- a per-message byte maximum enforced before `postMessage`; and
- a per-execution aggregate queued-byte maximum with a near-limit warning.

On overflow, terminate that exact Worker, wake its waiter with a typed error that
names the observed bytes, configured capacity, and how to raise it, and let the
pending driver submit `driver_failed` to the sidecar. Do not silently truncate,
drop oldest output, spill to client filesystem, or reuse the unbounded
`AgentSession.events` array.

Keep output identity channel-derived from the Worker route and verify every
message's control token/request/execution tuple before accounting it. A malicious
adapter must not be able to label output as another session.

### 5. Make the pending driver asynchronous and use a Rust-owned exact route

In `packages/browser/src/acp-pending-driver.ts`:

- make `pollAgentOutput` return a promise;
- make `createAcpPendingResponseDriver` return
  `(frame: Uint8Array) => Promise<Uint8Array>`;
- `await` stdout/stderr/exit without blocking the browser main thread;
- keep all ACP/frame inspection and construction behind wasm helpers; and
- preserve the current sidecar-owned timeout phase, abort, restart, stderr event,
  and origin-response restoration behavior.

Replace `pendingResponseProcessId` plus `bindPendingProcess` with:

```ts
pendingResponseRoute(frame: Uint8Array):
	| { processId: string; executionId: string }
	| null;
```

Do not bind to `lastAgentExecutionId`. That is incorrect as soon as two owners or
sessions start adapters before the first pending interaction completes.

In Rust:

1. Construct one `BrowserAcpExtension`, retain its existing diagnostics handle in
   `AgentOsBrowserSidecarWasm`, then register that same extension.
2. Add an owner-validating lookup on `BrowserAcpDiagnostics` for
   `(connection_id, wire_session_id, vm_id, process_id) -> execution_id`.
3. Add `pending_frames::pending_route` that decodes the pending process and exact
   VM ownership from the sidecar-written response.
4. Expose it as wasm `pendingResponseRoute`, returning only the two opaque IDs to
   TypeScript.

The lookup must reject a process route owned by a different connection/session/
VM, even if the process string matches. Rust already stores the complete owner in
`BrowserAcpExecution`; no protocol field or TypeScript map is needed.

After this change, delete from
`packages/browser/src/converged-execution-host-bridge.ts`:

- `SyncAgent`, `SyncAgentExecutor`, and `agentExecutor` options;
- `AgentSession`, line buffering, base64-to-text fake dispatch, and fake event
  arrays;
- `lastAgentExecutionId` and `pendingProcesses`; and
- the default no-op/synthetic `createWorker` path, replacing it with the exact
  existing-primary bind or service-Worker spawn described above.

What remains should only parse the JSON callback envelope emitted by wasm,
validate its fields, and delegate to the runtime-owned execution host.

### 6. Route public extension calls on the main thread

Add an asynchronous transport next to `PushFrameSidecarTransport` in
`packages/runtime-browser/src/converged-sync-bridge-handler.ts`. Share the same
frame encode/decode/rejected-response logic, but await `pushFrameAsync`.

Expose a method on `ConvergedExecutorSession` and `ConvergedServicer` such as:

```ts
dispatchExtensionRequest(
	namespace: string,
	payload: Uint8Array,
): Promise<Uint8Array>;
```

It must use the bootstrapped VM's exact ownership and return only the final
`ext_result`. Update `BrowserRuntimeDriver.dispatchExtensionRequest` at current
lines 1054-1064 to call that method instead of posting `type: "extension"` to the
guest Worker. Add the method to `NodeRuntimeDriver` so callers of
`createBrowserRuntimeDriverFactory` do not need an unsafe cast.

Keep the Worker's current rejection for direct extension control messages. ACP
control belongs to the trusted main-thread transport, not the untrusted guest
realm.

### 7. Complete sidecar-first teardown

Add `dispose()` to `ConvergedExecutorSession`/`ConvergedServicer`:

1. send `dispose_vm` with VM ownership;
2. send `close_session` with connection ownership; and
3. clear the local VM handle only after both terminal responses are validated.

`BrowserRuntimeDriver.dispose/terminate` must invoke that sidecar teardown before
terminating the ordinary Worker-controller instances. The Rust disposal hook
already closes ACP owners and calls `terminateWorker`; the runtime host must make
that callback idempotently remove and terminate the exact Worker route. If
sidecar cleanup and Worker cleanup both fail, throw an `AggregateError` retaining
both errors. Do not silently clear maps first or swallow worker termination
failures.

An ACP close-session request should release its adapter Worker immediately. Full
runtime disposal is the fail-safe for every remaining execution/context/VM route.

## Test migration and focused regressions

### Behavior-before tests

These should be committed first in the dedicated Item 73 revision and shown
failing against its parent:

1. In `packages/browser/tests/runtime-driver/converged-sidecar.test.ts`, replace
   the fake-agent success tests with a runtime-host spy and prove that calling the
   public factory without `agentExecutor` receives a Rust `createWorker` callback
   but starts no real Worker and produces no adapter output.
2. Add a browser fixture whose projected bin returns a sentinel different from
   every prebuilt worker. Run it through the old `async-kernel.worker.ts`; prove
   the result still comes from `AGENT_WORKERS`/`DEFAULT_AGENT_WORKER_URL`, even if
   the projected bin is changed to throw. This pins the substitution bug.
3. Retain the current unit assertions that `createWorker` only returns a generated
   ID and lifecycle callbacks are no-ops as the small historical proof. Delete or
   invert them only after the production host is installed.

### Behavior-after unit tests

Add focused runtime-browser tests for:

- one ordinary primary execution registering with exactly one Worker, followed
  by one ACP launch creating exactly one additional Worker;
- two simultaneous sidecar-managed Workers with distinct execution IDs, stdin,
  output, process config, and module caches;
- `execute-entrypoint` loading a module available only through
  `module.loadFile`, including the `/opt/agentos/bin/*` symlink;
- no main-thread read/fetch/source injection for that entrypoint;
- output chunks larger than 8,192 characters arriving intact;
- exact queue-cap overflow killing only the offending Worker with the typed
  capacity error;
- sidecar close and driver dispose leaving zero Worker routes and rejecting any
  outstanding wait exactly once; and
- a service Worker remaining alive beyond the generic persistent timeout until
  the sidecar terminates it (mark a real-time saturation version skipped; use an
  injected clock for the default suite).

Convert `packages/browser/tests/runtime-driver/acp-pending-driver.test.ts` to
async and retain its current coverage for multi-phase deadlines, stderr,
restart, cancellation, driver failure, exact origin ownership, and no internal
pending response. Add interleaved two-owner/two-process output proving the Rust
route selects the right execution. Merge the useful
`agent-drive-loop.test.ts` cases into this suite if `agent-drive-loop.ts` is
deleted.

Add Rust tests in `crates/agentos-sidecar-browser` proving:

- pending route lookup returns the execution belonging to the response's exact
  connection/session/VM;
- a same-named process under another owner is rejected;
- a released/aborted process has no route; and
- VM/session disposal leaves `sessions == 0`, `pending_interactions == 0`, and
  `process_routes == 0` through `BrowserAcpDiagnostics::resource_counts`.

Keep the existing `native-sidecar-browser` tests that assert the projected
entrypoint is passed unchanged and that exit/abort/dispose terminates the worker.
They already cover the Rust sidecar half of the boundary.

### Public browser E2E

Replace the path-mapped async harness with one Playwright gate built only from
public exports:

1. load a real built `.aospkg` (prefer
   `registry/agent/pi/dist/package.aospkg`); the existing Pi browser tests only
   prove a static/prebundled adapter source path, so they are useful runtime
   precedent but are not evidence that the projected package entrypoint runs;
2. pass its bytes to `createAgentOsConvergedSidecar` with no executor callback;
3. create the standard browser runtime factory/driver;
4. call the now-public extension dispatcher to list agents, create a session,
   and prompt it against the existing deterministic model endpoint;
5. assert the selected adapter file exists only in the projected VFS—there is no
   adapter bundle URL or host source fallback;
6. assert no `AcpPendingResponse` reaches the caller; and
7. close the ACP session and dispose the driver, then assert zero runtime Worker
   routes and zero Rust ACP routes/pending interactions.

If the full Pi prompt exposes a missing generic browser runtime primitive, fix
that primitive in this revision only when it is necessary to execute the
unchanged upstream adapter. Do not replace the adapter with a direct API stub or
fall back to the prebundled `pi-runner.ts` source-injection path. A small packed
echo fixture is useful for failure localization, but it does not replace the
required upstream-adapter gate.

After the public gate passes, delete or reduce:

- `AGENT_WORKERS` and `DEFAULT_AGENT_WORKER_URL` in
  `async-kernel.worker.ts`;
- the empty-bin generation in `buildBrowserAgentPackageFixtures`;
- synchronous fake-agent tests and fixtures; and
- any browser test that decodes/builds ACP continuation frames in TypeScript
  instead of using the wasm helpers.

Retain `SabRing`, `KernelReactor`, and their generic unit tests if other browser
runtime features still use them. Item 73 removes the fake ACP production path,
not independently useful bounded transport primitives.

## Risks and dependencies

- **Shared-checkout prerequisite:** the current checkout contains in-progress
  resumable pending-frame, timeout/abort, package-projection, and cleanup work.
  Item 73 should be based on those revisions after they are shaped; do not copy
  their changes into a second revision.
- **Multiple live adapters:** one Worker per execution is required. Reusing the
  driver's single Worker is unsafe because its process/module/stdin state is
  realm-global.
- **Primary-versus-service correlation:** ordinary execution registration must
  bind the existing primary Worker, while an unbound ACP launch allocates a new
  Worker. Treating every Rust `createWorker` callback as a new Worker request
  duplicates ordinary guest execution; treating every callback as pre-bound
  recreates the current ACP no-op.
- **Output integrity:** the present 8,192-character truncation is incompatible
  with ACP. The replacement must preserve bytes while still bounding queued
  transport state.
- **Lifecycle:** generic `persistent` carries a 120-second client timeout and is
  unsuitable for ACP services. The sidecar must remain the only lifetime owner.
- **Upstream runtime parity:** Pi is the best existing E2E candidate, but its
  current browser gate runs static/prebundled adapter source rather than the
  projected package entrypoint. Its real `.aospkg` is large, so keep the new
  projected-package proof in the explicit browser E2E rather than fast unit
  tests.
- **No native/Rust-client behavior change:** this is the browser execution-host
  seam. Do not add defaults or parallel ACP state to either SDK client.
- **No protocol schema change is required:** the exact execution mapping already
  exists in `BrowserAcpExecution` and can be exposed through the wasm helper.

## Exact changed-file inventory

The smallest implementation is expected to touch these production surfaces:

| File | Exact edit |
| --- | --- |
| `packages/browser/src/converged-sidecar.ts` | Remove `agentExecutor`; accept the runtime host in the loader; retain raw sync `pushFrame` and expose pending-aware `pushFrameAsync`. |
| `packages/browser/src/converged-execution-host-bridge.ts` | Delete the fake ACP implementation and maps; decode Rust callbacks and delegate exact execution/worker routes to the runtime host. |
| `packages/browser/src/acp-pending-driver.ts` | Make pending driving asynchronous and use the Rust-returned process/execution route. |
| `packages/runtime-browser/src/runtime-driver.ts` | Own the generic execution host; pre-bind the primary Worker; dispatch extensions on the main thread; terminate all Worker sessions. |
| `packages/runtime-browser/src/browser-worker-session.ts` (new) | Hold the extracted per-Worker control token, sync bridge, request/output routing, stdin/signal handling, bounds, and idempotent cleanup. |
| `packages/runtime-browser/src/worker-protocol.ts` | Add the internal projected-entrypoint execution request and lossless service-output messages. This is internal TS Worker messaging, not a shared wire-schema change. |
| `packages/runtime-browser/src/worker.ts` | Execute the exact projected path via the module servicer, preserve ACP output bytes, and omit the generic persistent timeout for sidecar-owned services. |
| `packages/runtime-browser/src/converged-driver-setup.ts` | Expose async extension dispatch and sidecar-first session/VM disposal on the bootstrapped servicer. |
| `packages/runtime-browser/src/converged-sync-bridge-handler.ts` | Add the separate async frame transport while leaving the synchronous syscall path unchanged. |
| `packages/runtime-browser/src/default-sidecar.ts` | Use the supplied runtime host instead of a no-op execution bridge. |
| `packages/runtime-browser/src/runtime.ts` and public index exports | Add the extension-dispatch contract and export only the host types needed by the browser package. |
| `crates/agentos-sidecar-browser/src/lib.rs` | Retain/access ACP diagnostics strongly enough to resolve exact pending process ownership. |
| `crates/agentos-sidecar-browser/src/pending_frames.rs` | Add the owner-validating pending process-to-execution route helper. |
| `crates/agentos-sidecar-browser/src/wasm.rs` | Register the retained extension and expose the opaque `{ processId, executionId }` helper through wasm-bindgen. |

Expected test and call-site edits:

- `packages/browser/tests/runtime-driver/acp-pending-driver.test.ts` and
  `converged-sidecar.test.ts` for async pending routing and fake-path deletion;
- runtime-browser unit tests plus its fake converged sidecar for primary binding,
  isolated service Workers, byte-preserving output, bounds, and teardown;
- `packages/browser/tests/browser-wasm/async-kernel.worker.ts`, the public
  converged runtime harness/spec, and
  `packages/browser/scripts/build-wasm-test-assets.mjs` for a real executable
  package fixture with no path map;
- `crates/agentos-sidecar-browser` unit tests for exact route ownership and
  cleanup; and
- public loader call sites reported by repository search, especially
  `packages/playground/frontend/runtime-harness.ts`, browser harness entries,
  and runtime-browser fake loaders.

`packages/browser/README.md` must drop the fake executor surface if it documents
it. `packages/browser/src/index.ts` and `packages/runtime-browser/src/index.ts`
need only export-shape updates. No shared protocol fixture regeneration should
be necessary. Regenerate the secure-exec compatibility mirror only if the
changed public exports are among its shimmed surfaces; verify that with the
mirror generator rather than hand-editing generated files.

## Focused validation

Run the narrow gates during implementation, then the public browser E2E after
the real package fixture is built:

```text
cargo test -p agentos-sidecar-browser
cargo test -p agentos-native-sidecar-browser
pnpm --filter @rivet-dev/agentos-runtime-browser check-types
pnpm --filter @rivet-dev/agentos-runtime-browser test:unit
pnpm --filter @rivet-dev/agentos-browser check-types
pnpm --filter @rivet-dev/agentos-browser exec vitest run tests/runtime-driver
pnpm --filter @rivet-dev/agentos-browser build:wasm-test-assets
pnpm --filter @rivet-dev/agentos-browser test:browser-wasm
```

The completion evidence must record the named behavior-before failures, the
focused after tests, and the public projected-`.aospkg` ACP list/create/prompt
gate. Type checks or the existing prebundled Pi gate alone do not complete Item
73.

## Dedicated JJ revision

Implement Item 73 in exactly one new stacked revision after its prerequisites,
with a description such as:

```text
fix(browser): execute projected ACP adapters
```

That revision owns the runtime-worker extraction, async frame seam, fake-path
deletion, Rust wasm route helper, migrated tests, and Item 73 tracker update. Do
not mix Items 71/72 or unrelated browser cleanup into it. The completion gate is
all three tracker checkboxes: behavior-before evidence, behavior-after public
factory E2E, and the dedicated revision with Item 73 marked `done`.
