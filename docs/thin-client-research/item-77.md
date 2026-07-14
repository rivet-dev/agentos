# Item 77 research: make pooled native-child shutdown linearizable and reaped

Status: implementation-ready research only. This note does not modify
production code, tests, or the Item 77 tracker status.

Inspected on **2026-07-14** at revision **`2fbfc0826774`**. Tracker anchors are
`docs/thin-client-migration.md:123` (issue inventory), current line 205
(pending status), and current line 292 (before/after/complete checklist).

## Recommendation

Implement one host-owned lifecycle for each pooled sidecar generation in both
clients:

1. serialize VM creation through successful lease registration against explicit
   sidecar disposal;
2. keep a disposing pool generation in the cache until its native child has
   actually exited and been reaped;
3. route explicit disposal, watchdog shutdown, startup rollback, transport
   drop, and Rust fail-closed route shutdown through one idempotent child
   termination primitive; and
4. publish `disposed` and admit a replacement generation only after that
   primitive confirms exit.

Rust needs a child-supervisor task on Item 76's process-owned transport runtime.
The supervisor, rather than an individual caller future, owns
`tokio::process::Child` through `start_kill` and bounded `wait`. TypeScript needs
the equivalent retryable termination promise in `StdioSidecarProcess`; it must
distinguish a signal exit from a timeout, reject `child.kill(...) === false`,
and reject when the post-`SIGKILL` deadline expires.

Priority: **P1**. Root-cause confidence: **high**. Fix confidence: **high** once
Items 47 and 76 are parents of the revision.

This work belongs in the clients' native host transport and pool bookkeeping.
An OS parent is the only component that can retain and reap its child process;
the sidecar cannot acknowledge its own host-process death over the protocol.
No VM default, runtime policy, filesystem behavior, permission rule, or guest
process behavior moves into either client.

## Original issue

### Rust drops the only child handle before reaping

`SidecarTransport` owns the spawned Tokio child in
`crates/sidecar-client/src/transport.rs:542-566`. Its entire shutdown primitive
is currently `kill_child` at lines 803-810:

```rust
if let Some(mut child) = self.child.lock().take() {
    if let Err(error) = child.start_kill() {
        tracing::error!(?error, "failed to kill child sidecar process");
    }
}
```

Taking the `Child` before `start_kill` means every outcome loses ownership:

- a successful signal is not followed by `Child::wait`, so the host cannot
  prove the process was reaped;
- a signal error is only logged and the child cannot be retried;
- cancellation cannot be made safe because there is no retained state to
  resume; and
- two callers do not converge on one result: the first takes the handle and the
  second silently sees `None`.

The silence watchdog at `transport.rs:1201-1222` calls that same lossy function
and then fails pending requests. A watchdog racing explicit disposal can win
the `take()`, leaving disposal unable to confirm termination.

Rust has two more callers:

- `AgentOsSidecar::kill_connection` at
  `crates/client/src/sidecar.rs:199-206` takes the cached connection before the
  transport kill, so a later retry has neither the connection nor the child;
- `abort_wire_process_after_route_failure` at
  `crates/client/src/process.rs:753-763` invokes it when a guest process cannot
  be killed after host event-route loss.

`AgentOs::shutdown` at `crates/client/src/agent_os.rs:517-538` releases the last
lease, calls `kill_connection`, then calls `AgentOsSidecar::dispose`. Cancellation
after the first call leaves the handle in `ready`, with no connection and no
owned child. A retry can publish `disposed`, but it still cannot wait for the
old OS process.

### Rust create and dispose have no common linearization point

`AgentOs::create` resolves a pooled handle at
`crates/client/src/agent_os.rs:202-210`, establishes/reuses its process at lines
253-275, initializes the VM at lines 287-347, and increments
`active_vm_count` only at lines 355-359.

`AgentOsSidecar::dispose` at `crates/client/src/sidecar.rs:230-278` uses atomics
but no lifecycle lock. It sets `disposing`, unconditionally zeros the active
count, publishes `disposed`, and removes the pool entry without terminating or
reaping `connection`. It also does not actually dispose the active leases its
comment claims to drain.

The gaps are observable:

```text
create B                              shutdown/dispose A
--------                              ------------------
reuse old connection
open session / initialize VM
                                      observe active_vm_count == 0
                                      take connection; start_kill; lose Child
                                      publish disposed; remove pool entry
increment active_vm_count to 1
return VM B backed by the dead old generation
```

Another ordering lets `ensure_connection` install a new child on the old handle
while disposal is publishing it as disposed. `ensure_connection` at
`sidecar.rs:148-197` checks only whether `connection` is populated; it does not
check the sidecar lifecycle state. Pool lookup at lines 398-403 also returns a
`disposing` handle because it excludes only `disposed`.

### TypeScript can resolve disposal without confirmed exit

The native child wrapper is in `packages/runtime-core/src/process.ts`.
`StdioSidecarProcess.waitForExit` at lines 100-129 returns `number | null`.
That representation conflates two different outcomes:

- timeout returns `null`; and
- a real signal exit invokes Node's `exit` listener with `code === null`, so it
  also returns `null`.

`StdioSidecarProtocolClient.dispose` at
`packages/runtime-core/src/native-client.ts:164-213` waits for graceful exit,
sends `SIGKILL` after a timeout, and then ignores the result of the second
`waitForExit` at line 184. It also ignores Node's boolean `child.kill` result;
`false` normally means no signal was sent and does not throw. After either
failure it destroys stdio and returns success.

The silence callback at `native-client.ts:75-84` repeats the raw, swallowed
`SIGKILL`. Shared-process authentication rollback at
`packages/core/src/agent-os.ts:3377-3392` does the same before clearing the
tracked child/promise. These paths can all abandon a still-running child.

The synchronous `process.on("exit")` fallback at `agent-os.ts:3257-3279` is
necessarily best effort because Node is already exiting. Keep it as the final
process-exit fallback, but do not use its limitations to justify best-effort
shutdown in an awaited API.

### TypeScript has the same create/dispose race

The retained host state is at `packages/core/src/agent-os.ts:3214-3240`.
`leaseAgentOsSidecarVm` checks `ready` synchronously at lines 3548-3553 and
takes an event-loop hold before awaiting creation, but it does not add the
lease to `activeLeases` until lines 3624-3629.

`AgentOsSidecar.disposeOnce` at lines 3466-3488 sets `disposing` and snapshots
only the leases already in the set. A create paused between the state check and
lease insertion is invisible. Disposal can terminate the shared process and
publish `disposed`; the create then inserts a new active lease into the disposed
handle. The event-loop hold protects Node ref/unref accounting, not lifecycle
serialization.

`nativeProcess?: Promise<SharedSidecarNativeProcess>` also hides the concrete
client until authentication completes. Disposal and failed-auth cleanup should
retain the spawned generation immediately, not recover it only from a fulfilled
promise.

## Exact cross-client behavior

The clients should expose the same state contract:

- a create that enters the lifecycle gate first may finish and register its
  lease; a later explicit dispose must then dispose that lease;
- once explicit disposal enters first, new creates against that handle fail as
  `disposing` and pool lookup does not return it as reusable;
- concurrent dispose callers await the same work;
- a termination failure leaves state `disposing`, retains the exact child
  generation, and is retryable;
- `disposed` means every authoritative VM close attempted by the handle has
  completed and the owned native child has produced a terminal status through
  `wait`/Node `exit`;
- the pool admits a replacement only after the prior exact instance reaches
  `disposed`; and
- absent an awaited caller, a watchdog/drop-triggered termination continues in
  its host supervisor and reports any failure visibly.

Do not add a protocol `shutdown` acknowledgement for OS-process death. Existing
session/VM close responses remain the authoritative runtime cleanup
acknowledgements; OS exit/reaping is a separate host responsibility.

## Exact Rust edits

### `crates/sidecar-client/src/transport.rs`

Build on Item 76's owned transport runtime. Replace
`parking_lot::Mutex<Option<Child>>` with a private, bounded child supervisor:

1. Move the spawned `Child` into one supervisor task as soon as stdin/stdout are
   taken. The task must run on Item 76's process-lifetime runtime, not on an
   arbitrary caller runtime.
2. Give the transport a bounded termination-command sender and a shared terminal
   result. A command must be enqueued before returning the awaitable result, so
   dropping/cancelling that result cannot cancel the supervisor operation.
3. On each command, call `try_wait` first. If already exited, record the status.
   Otherwise call `start_kill`, then run `Child::wait` under one fixed, bounded
   force-exit deadline.
4. Do not drop the child on signal error or wait timeout. Return a typed
   termination failure while the supervisor retains it, allowing the next
   command to retry.
5. Once `wait` succeeds, cache a small cloneable terminal record (exit code and
   Unix signal) and answer every later termination request with that same
   record. Exactly one task performs the reap.
6. Have the reader's EOF tail and `Drop for SidecarTransport` enqueue a
   termination/reap request. Drop cannot await, but Item 76's owned runtime lets
   the supervisor continue after the transport/caller disappears.

Recommended API shape (names may vary, semantics may not):

```rust
pub async fn terminate_child(&self) -> Result<ChildExit, TransportError>;
fn request_child_termination(&self) -> ChildTermination;
```

`ChildTermination` is the cancellation-safe awaitable returned after the
bounded command is accepted. Keep `kill_on_drop(true)` only as a final runtime
destruction fallback; it is not the successful disposal path.

Change `run_silence_watchdog` at current lines 1204-1222 to await
`terminate_child`. Always fail pending requests with the original watchdog
failure. If termination itself fails, emit a structured error with its typed
phase; do not silently replace or discard either failure.

Change the fail-closed route callback in
`crates/client/src/process.rs:753-763,903-921` to await the same termination
primitive. The helper can become async because its production caller is already
async. Remove every call to the old `kill_child` API.

### `crates/sidecar-client/src/error.rs` and Rust public mapping

Add a typed transport termination error rather than another unstructured
string. It needs at least a stable phase (`signal` or `reap_timeout`), the
bounded deadline where relevant, and the underlying message. Map it explicitly
through `crates/client/src/error.rs:76-82` to a typed `ClientError` termination
variant so public `AgentOsSidecar::dispose` callers can distinguish termination
from a sidecar wire rejection.

The matching TypeScript error is described below. Keep message semantics equal:

```text
sidecar termination failed during signal: <cause>
sidecar termination failed during reap_timeout: no exit after <N>ms following SIGKILL
```

No errno or sidecar response code is involved.

### `crates/client/src/sidecar.rs`

Add one async lifecycle gate to `AgentOsSidecar`. Its protected operation is
small in concept but spans creation through lease registration and explicit
disposal through confirmed reaping.

Replace count-only leases with exact host lease records. The smallest Rust
representation is a generated lease ID plus a `Weak<AgentOsInner>` retained by
the sidecar. This avoids an `Arc` cycle while allowing public sidecar disposal
to upgrade and shut down every live VM. Derive `active_vm_count` from the
records (or update the existing atomic only under the same transitions); never
zero it independently of real lease removal.

Refactor these methods:

- `ensure_connection` (current lines 148-197) may run only for a `ready` handle
  under the lifecycle gate;
- `kill_connection` (199-206) becomes retryable `terminate_connection` and does
  not remove `SharedConnection` until `terminate_child().await` succeeds;
- `dispose` (230-278) serializes concurrent callers, sets `disposing`, drains
  exact live leases, awaits connection termination, then and only then sets
  `disposed` and removes the exact pool entry;
- lease disposal (281-310) removes its exact record once and reports the
  remaining count; and
- `get_shared_sidecar` (390-443) returns only `ready` entries. If it sees
  `disposing`, release the process-global map lock, await/retry that exact
  handle's disposal, then loop. Never hold `SHARED_SIDECAR_POOL_LOCK` across an
  await and never install a second generation beside a disposing one.

Preserve the existing exact-instance pool removal check. Add a private
`dispose_if_idle` for Rust's current last-lease behavior: after taking the
lifecycle gate it must re-check that no create registered a lease. Public
`dispose` drains leases; `dispose_if_idle` returns without terminating when a
concurrent create linearized first.

### `crates/client/src/agent_os.rs`

In `AgentOs::create`, serialize only after all explicit input has been validated
and serialized, but before `ensure_connection`. Hold the sidecar lifecycle gate
through open-session/VM initialization and insertion of the exact weak lease
record. This gives disposal one unambiguous before/after point without moving
configuration logic into the client.

Keep the existing sidecar-owned initialization transaction and failure rollback.
Do not add a client VM state machine. A cancelled create drops the lifecycle
guard; any response that establishes a session must continue to use retained
transport response correlation/sidecar idempotent close rather than a detached
client registry.

In `shutdown` at current lines 517-538, release the exact lease and call
`dispose_if_idle` only when the release reports zero leases **and** the sidecar
is still `ready`. When public sidecar disposal has already set `disposing`, it
owns final termination; the nested VM shutdown must not reacquire the same gate
and deadlock. The shutdown attempt remains incomplete until whichever owner is
responsible confirms child reaping. If its caller is cancelled while awaiting
termination, the child supervisor continues; the next idempotent shutdown
awaits the same result.

## Exact TypeScript edits

### `packages/runtime-core/src/process.ts`

Change `waitForExit` to return a terminal record or a distinct timeout:

```ts
interface SidecarExitStatus {
  exitCode: number | null;
  signal: NodeJS.Signals | null;
}

waitForExit(timeoutMs: number): Promise<SidecarExitStatus | null>
```

An `exit`/`close` event with `code === null` and a signal is a non-null terminal
record. Only expiry of the timer returns `null`.

Add one retryable `terminate` method/promise to `StdioSidecarProcess`:

- wait for the requested graceful interval when disposal closed stdin;
- re-check terminal metadata before and after signaling;
- treat `child.kill("SIGKILL") === false` as `signal` failure unless the child
  concurrently reached a terminal state;
- after a successful signal, require a non-null exit record within the force
  deadline;
- on signal failure or timeout, clear only the attempt promise and retain the
  child/stdio for retry; and
- cache success so watchdog, disposal, and rollback converge on one exit.

### `packages/runtime-core/src/native-client.ts` and `sidecar-errors.ts`

Add `SidecarTerminationError` with the same `signal | reap_timeout` phase and
message contract as Rust.

Refactor `StdioSidecarProtocolClient.dispose` at current lines 164-213 to call
the process termination primitive. Destroy protocol streams only after exit is
confirmed. Preserve the existing nonzero natural-exit diagnostic, but do not
mistake a signal status for a timeout.

The silence callback at lines 75-84 must start that same force-termination
attempt. The pending request still receives `SidecarSilenceTimeout`; a
termination failure is stored for a later `dispose` retry and reported to
stderr immediately. There must be no empty catch or floating rejected promise.

### `packages/core/src/agent-os.ts` after Item 47

Item 47 removes the synthetic `AgentOsSidecarClient` state machine and leaves
the real VM-admin lease, `activeLeases`, native process, event-loop holds, and
public sidecar state. Put the Item 77 lifecycle gate around that direct lease;
do **not** add the deleted session/VM maps back.

Use a small FIFO promise gate stored in `AgentOsSidecarState`. Run the direct
`createVmAdmin` factory through successful `activeLeases` insertion under that
gate. Run `AgentOsSidecar.disposeOnce` through the same gate. JavaScript promises
are not caller-cancellable, so the operation continues once enqueued.

Change the native generation from an opaque fulfilled-only promise to an
immediately retained record, for example:

```ts
interface SharedSidecarNativeGeneration {
  client: SidecarProcess;
  session: Promise<AuthenticatedSession>;
}
```

This lets authentication rollback and concurrent disposal terminate the exact
spawned client even before authentication resolves. Clear the generation and
`sharedChild` only after `client.dispose()` confirms process exit. On failure,
retain both, leave the handle `disposing`, and let the next dispose call retry.

Make `getSharedAgentOsSidecarInternal` and `resolveAgentOsSidecar` async (their
public callers already return promises). Never return a `disposing` handle as
reusable and never replace it in `sharedSidecars` before confirmed disposal.

Keep `eventLoopHolds`: it is valid Node host state. The lifecycle gate prevents
dispose/create races; the hold still independently controls whether live work
keeps Node's event loop referenced.

## Deterministic before tests

Record failures against Item 77's parent before editing production code.

### Rust

In the `agentos-sidecar-client` transport unit tests, spawn a long-lived child,
install it in the current transport, call `kill_child`, and assert the transport
still owns a waitable child until reaping completes. The parent fails
immediately because `child.lock().is_none()` as soon as `kill_child` returns.
This is deterministic source behavior; do not use a sleep as proof of a zombie.

Add a real-client lifecycle regression with a unique pool and a blocking
`sidecar_js_bridge_callback` during `InitializeVm`:

1. start VM B creation and wait until the callback proves initialization is in
   flight but the lease count is still zero;
2. start disposal of the same explicit/shared sidecar;
3. assert it cannot publish `disposed` or remove/install a pool generation;
4. release the callback and observe the chosen linearization; and
5. prove no returned VM is attached to a killed/disposed generation.

The current implementation publishes `disposed` while creation is paused and
can later increment the disposed handle's count.

### TypeScript

In `packages/runtime-core/tests/process.test.ts`, use
`StdioSidecarProcess.fromChild` with an EventEmitter-backed fake child. Emit
`exit(null, "SIGKILL")` and assert `waitForExit` reports a terminal signal
record. The current `number | null` API returns the same `null` used for timeout.

In `packages/runtime-core/tests/native-client.test.ts`, run a fixture that stays
alive after stdin EOF, stub `child.kill` to return `false`, and use short test
grace/force deadlines. Assert `dispose()` rejects and the child is still owned.
The current implementation resolves success. Restore the real kill in `finally`
so the test itself cannot leak a process.

Add a controlled direct-VM factory regression after Item 47: pause creation
before lease insertion, call `sidecar.dispose()`, and assert `disposed` is not
published while the create is invisible. The current code snapshots an empty
`activeLeases`, disposes the process, and later inserts into the disposed state.

## After tests

### Rust transport

Add focused `crates/sidecar-client` tests for:

- dropping a termination awaiter after its command is accepted, then awaiting a
  second request and observing the same reaped terminal record;
- watchdog and explicit termination commands overlapping, with one signal/reap
  and equal completion;
- injected signal failure and reap timeout retaining the child for a successful
  retry; and
- reader EOF and transport drop enqueuing supervisor cleanup.

Use a small private child-driver test seam for the otherwise unforceable OS
failure branches. Do not add a public mock process API or poll `/proc` as the
definition of reaping; successful `Child::wait` is the proof.

### Rust pool/client

Add `crates/client/tests/sidecar_lifecycle_e2e.rs` using the blocking bridge
callback above. Cover both orderings:

- create linearizes first: it registers one exact lease, then explicit disposal
  closes it and reaps the child before `Disposed`;
- dispose linearizes first: a later create is rejected while disposing and a
  replacement pool handle appears only after the old child is reaped.

Cancel/drop the first termination waiter after the supervisor accepts the
command and prove retry completes. Assert the pool contains no old-generation
entry and no child remains owned after success.

Keep Item 76's two-runtime regression and `sidecar_pool_e2e` green.

### TypeScript

Extend `process.test.ts` and `native-client.test.ts` to cover:

- graceful numeric exit, signal exit, and true timeout as distinct results;
- `kill() === false`, thrown kill, and force-exit timeout as typed retryable
  failures;
- retry after failure, with streams/generation retained until exit; and
- silence-watchdog and explicit dispose convergence on one terminal result.

After Item 47, add `packages/core/tests/sidecar-lifecycle.test.ts` (or extend its
direct-lease acceptance test) with a deferred real-admin factory. Assert create
and dispose order, exact lease count, no second generation while disposing, and
`disposed` only after the fake child exit promise resolves. Do not restore the
synthetic lifecycle classes merely to make the test injectable.

Retain the real placement, disposal-retry, sibling-ownership, and clean-exit
suites.

## Risks and guardrails

- **Item 76 runtime ownership is mandatory.** A perfect supervisor spawned on a
  caller's Tokio runtime is still aborted when that runtime exits.
- **Never hold the process-global Rust pool lock across await.** Clone the exact
  disposing handle, release the lock, await it, then retry lookup.
- **No `Arc` cycle.** Rust sidecar lease records must be weak references; the VM
  already strongly owns its sidecar handle.
- **No replacement-before-reap.** Removing the cache entry on `disposing`,
  signal send, stdin close, or timeout can run two native generations for one
  pool.
- **Retain ownership on failure.** Do not clear Rust `SharedConnection`, the
  supervisor child, TypeScript `nativeProcess`, `sharedChild`, or stdio until
  terminal exit is confirmed.
- **Bound every wait.** Grace and force deadlines remain fixed host-transport
  constants. A timeout is a typed failure and retry point, not permission to
  claim success.
- **Preserve original failures.** Watchdog requests still fail with silence
  timeout and startup still reports authentication/creation failure; append or
  aggregate termination failure without replacing the initiating error.
- **Process-exit hook exception.** Node's synchronous host-exit hook cannot
  await. It remains a final best-effort `SIGKILL`, but no awaited API may copy
  that behavior.
- **Do not add PID polling.** Use Node `exit`/`close` and Tokio `Child::wait`,
  matching native Linux parent/child behavior.
- **Do not broaden into guest process policy.** This item terminates the one
  trusted native sidecar child. Guest `KillProcess`, ACP adapter escalation,
  and VM resource cleanup remain sidecar-owned.

## Dependencies and revision boundary

- **Item 47 must land first.** It removes TypeScript's synthetic lifecycle.
  Implement the gate around the direct real-VM admin and retained host lease
  set. Reintroducing `AgentOsSidecarClient`, fake IDs, or VM maps is incorrect.
- **Item 76 must land first.** Its process-owned Tokio runtime is where the Rust
  child supervisor runs. Item 77 supersedes Item 76's temporary instruction to
  leave `kill_child` unchanged, but does not alter its bounded runtime design or
  two-runtime acceptance test.
- **Item 74 is adjacent but independent.** A process-event-pump failure may
  abort a guest process; it does not own the native sidecar child lifecycle.
- Existing Rust shutdown retry and TypeScript VM disposal retry semantics must
  remain. Item 77 extends retry through native-child reaping.

Use one dedicated stacked JJ revision after Items 47 and 76, for example:

```text
fix(client): confirm pooled sidecar termination
```

Expected production/test paths:

- `crates/sidecar-client/src/error.rs`
- `crates/sidecar-client/src/transport.rs`
- `crates/client/src/error.rs`
- `crates/client/src/sidecar.rs`
- `crates/client/src/agent_os.rs`
- `crates/client/src/process.rs`
- `crates/client/tests/sidecar_lifecycle_e2e.rs`
- `packages/runtime-core/src/process.ts`
- `packages/runtime-core/src/native-client.ts`
- `packages/runtime-core/src/sidecar-errors.ts`
- `packages/runtime-core/tests/process.test.ts`
- `packages/runtime-core/tests/native-client.test.ts`
- `packages/core/src/agent-os.ts`
- `packages/core/tests/sidecar-lifecycle.test.ts`
- `docs/thin-client-migration.md` for Item 77 evidence/status only

No native-sidecar implementation, wire schema, generated binding, VM runtime,
VFS, package, ACP, actor, or website file belongs in this revision.

## Validation

```bash
cargo build -p agentos-sidecar
cargo test -p agentos-sidecar-client --lib -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test sidecar_lifecycle_e2e -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test shared_sidecar_runtime_e2e -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test sidecar_pool_e2e -- --nocapture

pnpm --dir packages/runtime-core exec vitest run \
  tests/process.test.ts tests/native-client.test.ts --reporter=verbose
pnpm --dir packages/core build
AGENT_OS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/core exec vitest run \
    tests/sidecar-lifecycle.test.ts \
    tests/sidecar-placement.test.ts \
    tests/agent-os-dispose-retry.test.ts \
    tests/shared-sidecar-ownership.test.ts \
    tests/shared-sidecar-clean-exit.test.ts --reporter=verbose

cargo check --workspace
cargo fmt --all -- --check
pnpm build
pnpm check-types
git diff --check
```

Run the cancellation/overlap tests repeatedly while implementing; their
coordination must use barriers/deferred promises, not scheduling sleeps.
