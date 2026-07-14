# Item 57 research: make process-exit callbacks result-bearing

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 57 tracker status.

## Recommendation

Change `on_process_exit`/`onProcessExit` from a success-only exit-code callback
to a once-only terminal-outcome callback:

- Rust receives `Result<i32, ClientError>`;
- TypeScript receives
  `{ ok: true; exitCode: number } | { ok: false; error: Error }`.

Deliver the same retained outcome to a subscriber registered before or after
terminal observation. Unknown PID remains an immediate method error. Never map a
lost/closed route or a sidecar-reported terminal failure to a made-up exit code.

The Rust edit must also stop discarding the optional `RejectedResponse` carried
by `ProcessExitedEvent.error`. TypeScript already rejects `ManagedProcess.wait()`
for that wire shape; Rust currently publishes the frame's numeric `exit_code` as
success. A result-bearing callback without this mapper fix would still leave the
two clients observably different.

Priority: **P2**. Confidence: **high**.

This remains host-side client functionality. The sidecar already emits the real
terminal event and owns process lifecycle; only the client can invoke a caller's
Rust closure or JavaScript function. No callback logic or test should move into
the sidecar.

## Original issue

The tracker entries are at `docs/thin-client-migration.md:103,190,282`:

> Rust `on_process_exit` accepts only `FnOnce(i32)`, so a route failure can be
> logged but cannot reach that callback without inventing an exit code. Add a
> result-bearing/error callback and mirror it in TypeScript.

Item 22 already made event-route loss terminal and typed. Item 29 already made
terminal success/failure retention bounded. `wait_process`/`waitProcess` already
surface the correct outcome. Item 57 closes the remaining hole at the callback
API: callbacks are currently notified only on success.

## Exact current behavior

### Rust stores the typed failure, then logs it instead of calling the handler

The authoritative client-side terminal state is already expressive:

- `crates/client/src/agent_os.rs:50-56` defines internal `ProcessExit` as
  `Exited(i32)`, `EventStreamLagged { skipped }`, or `EventStreamClosed`;
- `crates/client/src/process.rs:785-854`, `AgentOs::run_spawn_events`, writes
  lag/closed failures into the per-process `watch` channel and force-aborts a
  process after lag;
- a real matching sidecar terminal event writes `Exited(exit_code)` at
  `process.rs:831-833`; and
- `process_exit_result` at `process.rs:988-998` already converts all three
  states into `Result<i32, ClientError>` for `wait_process`.

The public callback discards that expressiveness. `AgentOs::on_process_exit` at
`crates/client/src/process.rs:445-497` accepts only:

```rust
handler: impl FnOnce(i32) + Send + 'static
```

Both its already-terminal branch (lines 460-474) and asynchronous branch (lines
478-495) invoke the handler only for `ProcessExit::Exited`. Lag and closure are
written to tracing and the `FnOnce` is dropped without invocation.

There is one additional silent edge: if `rx.changed()` returns channel closure
without a retained `ProcessExit`, the loop at lines 478-495 simply ends and
drops the handler. That must also become an `EventStreamClosed` callback result.

The existing `wait_process` implementation at lines 501-519 is the semantic
reference for retained states: success returns the code and retained route
failure returns its typed `ClientError`. Its defensive raw-watch-close branch is
still a generic `ClientError::Sidecar`; Item 57 should make that branch the same
typed `EventStreamClosed { context: "process exit" }` delivered to a callback.

### Rust additionally drops the protocol's terminal error

The authoritative BARE shape at
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:969-975` is:

```text
type ProcessExitedEvent struct {
  processId: str
  exitCode: i32
  stdout: optional<data>
  stderr: optional<data>
  error: optional<RejectedResponse>
}
```

`RejectedResponse` is exactly `{ code: str, message: str }` at lines 809-812.
The error is not an alternate event and it does not remove `exitCode`; consumers
must check the optional error before treating the integer as success.

Finite Rust `exec` already does this correctly in `apply_exec_event` at
`crates/client/src/process.rs:930-965`: `Some(error)` becomes
`ClientError::Kernel { code, message }`. The spawned-process pump does not. Its
`ProcessExitedEvent` match at lines 831-833 ignores `exited.error` and stores
only `ProcessExit::Exited(exited.exit_code)`. Consequently both
`wait_process` and a future result-bearing `on_process_exit` can report `Ok(code)`
for a terminal frame that the sidecar marked failed.

Represent this internally as a cloneable `ProcessExit::Failed { code, message }`
and map it to the existing public `ClientError::Kernel`. Do not store a generic
string and do not invent a new protocol error taxonomy.

### TypeScript has the callback defect but already handles terminal wire errors

TypeScript already retains a compact union in
`packages/core/src/agent-os.ts:1268-1289`:

```ts
type ProcessRoute =
	| { state: "running"; /* host routes */ }
	| { state: "exited"; exitCode: number }
	| { state: "failed"; error: Error };
```

But `RunningProcessRoute.exitHandlers` is a
`Set<(exitCode: number) => void>`. `_trackProcess` at
`agent-os.ts:1642-1704` behaves as follows:

- successful `proc.wait()` snapshots and invokes handlers with the exit code;
- rejected `proc.wait()` clears every exit handler, stores `{ state: "failed",
  error }`, and only calls `console.error`.

`onProcessExit` at `agent-os.ts:1785-1799` invokes late success subscribers
synchronously, but throws a retained failed-route error from the registration
call instead of delivering it to the callback. A subscriber registered while
the process is running receives no call if the route later fails.

`waitProcess` at lines 1801-1808 already resolves/rejects from the same retained
route and is the TypeScript semantic reference.

The source of a failed route is intentionally broader than event-route loss.
`NativeSidecarKernelProxy.runEventPump` at
`packages/core/src/sidecar/rpc-client.ts:835-846` checks the converted
`process_exited.error_code`, constructs an `Error` with the authoritative code
and message, and calls `failProcess`; `ManagedProcess.wait()` then rejects with
that exact object. Runtime-core's live shape is at
`packages/runtime-core/src/event-buffer.ts:32-40`, and its conversion from the
generated optional `RejectedResponse` is at lines 380-397.

Therefore `_trackProcess`'s rejected `proc.wait()` path covers transport/event
pump failures and sidecar terminal failures alike. Item 57 must forward the exact
retained `Error` to early and late callbacks. It must not parse `error.code` or
try to infer whether the cause was transport, capture policy, or guest execution.

## Public outcome contract

### Rust

In `crates/client/src/process.rs`, add a public alias near the other supporting
types:

```rust
/// Once-only terminal outcome delivered by `on_process_exit`.
pub type ProcessExitResult = std::result::Result<i32, ClientError>;
```

Change the handler parameter to:

```rust
handler: impl FnOnce(ProcessExitResult) + Send + 'static
```

Re-export `ProcessExitResult` from `crates/client/src/lib.rs` beside
`SpawnHandle` and `SpawnOptions`.

Using the SDK's existing `ClientError` is important. `EventStreamLagged` retains
its `skipped` count and `EventStreamClosed` retains context; a string or optional
integer would lose that information. `ClientError` need not become `Clone`:
internal `ProcessExit` remains cloneable for each watch subscriber, and every
callback independently maps its cloned state to one owned error.

### TypeScript

In `packages/core/src/runtime.ts`, add an exported discriminated result near
`ManagedProcess`:

```ts
export type ProcessExitResult =
	| { ok: true; exitCode: number }
	| { ok: false; error: Error };
```

Import it into `packages/core/src/agent-os.ts`, use it for
`RunningProcessRoute.exitHandlers`, `_trackProcess`, `spawn`, and
`onProcessExit`, and export it from `packages/core/src/index.ts`.

The discriminated union prevents impossible states: failure has no fabricated
code and success has no optional error. Do not use `number | null`, `-1`, `0`,
or `1` as a failure marker. Do not use an error-first pair with two nullable
arguments.

The error should be the exact retained `Error` object. Do not replace it with a
new anonymous wrapper. Item 63 later upgrades terminal code-bearing failures to
an exported structured error; forwarding the exact object here makes that
upgrade automatically visible to callback consumers.

## Exact production edits

### `crates/client/src/agent_os.rs`

Extend the private, cloneable terminal union:

```rust
pub(crate) enum ProcessExit {
    Exited(i32),
    Failed { code: String, message: String },
    EventStreamLagged { skipped: u64 },
    EventStreamClosed,
}
```

Update the `ProcessEntry.exit_tx` comment from “`Some(code)`” to “a terminal
outcome.” This is client correlation state, not client-owned policy.

### `crates/client/src/process.rs`

Add a small pure mapper used by `run_spawn_events` and its unit tests:

```rust
fn process_exit_from_event(exited: &wire::ProcessExitedEvent) -> ProcessExit {
    match &exited.error {
        Some(error) => ProcessExit::Failed {
            code: error.code.clone(),
            message: error.message.clone(),
        },
        None => ProcessExit::Exited(exited.exit_code),
    }
}
```

The matching event branch sends this mapped value instead of unconditionally
sending `Exited(exited.exit_code)`. Extend `process_exit_result` with:

```rust
ProcessExit::Failed { code, message } => Err(ClientError::Kernel { code, message }),
```

This matches finite `exec` and TypeScript `ManagedProcess.wait()` without
changing the sidecar or wire schema.

Refactor the duplicate immediate/asynchronous match into one private helper that
takes the subscribed `watch::Receiver` and the result-bearing `FnOnce`. The
helper is also the deterministic unit-test seam:

```rust
fn subscribe_process_exit(
    mut rx: watch::Receiver<Option<ProcessExit>>,
    handler: impl FnOnce(ProcessExitResult) + Send + 'static,
) -> Subscription {
    if let Some(exit) = rx.borrow().clone() {
        handler(process_exit_result(exit));
        return Subscription::noop();
    }

    let task = tokio::spawn(async move {
        loop {
            match rx.changed().await {
                Ok(()) => {
                    let Some(exit) = rx.borrow().clone() else {
                        continue;
                    };
                    handler(process_exit_result(exit));
                    return;
                }
                Err(_) => {
                    handler(Err(ClientError::EventStreamClosed {
                        context: "process exit",
                    }));
                    return;
                }
            }
        }
    });
    Subscription::new(move || task.abort())
}
```

`pid` is not needed once tracing-only failure branches are removed. The callback
itself is now the primary failure delivery path. If callback panics, normal Rust
task panic reporting applies; do not catch panics or invent callback policy.

Then make `AgentOs::on_process_exit` perform only the PID lookup/subscription and
delegate to the helper. Preserve these semantics:

- unknown PID returns `ClientError::ProcessNotFound` without invoking the
  callback;
- a retained outcome invokes synchronously and returns `Subscription::noop()`;
- a live outcome invokes exactly once;
- dropping the returned subscription before terminal observation aborts the
  task and does not invoke the callback; and
- success, lag, close, and unexpected watch-channel closure are mutually
  exclusive outcomes.

Use `ClientError::EventStreamClosed { context: "process exit" }` for an
unexpected closed watch in both `subscribe_process_exit` and `wait_process`, so
the callback and future APIs agree on the same typed terminal outcome.

Update stale comments at `process.rs:305-307`, `agent_os.rs:68-69`, and the
method docs at `process.rs:445-448` so they say “terminal outcome,” not
`Some(code)`/exit code only.

Do not change the sidecar protocol. Do not alter event ownership/process-ID
filtering, capture semantics, or route-failure cleanup. The necessary Rust
changes are limited to preserving data already present in the received terminal
event and delivering existing client route failures to the host callback.

### `packages/core/src/agent-os.ts`

Change every exit-handler set/signature from `(exitCode: number) => void` to
`(result: ProcessExitResult) => void`.

In `_trackProcess`'s success branch, keep compact route replacement and pruning
before callback delivery, then invoke the captured handlers with:

```ts
{ ok: true, exitCode: code }
```

In the rejection branch, normalize the rejection exactly once, snapshot the
handlers before clearing them, store the same error in the compact failed route,
prune, and invoke each captured handler with:

```ts
{ ok: false, error: routeError }
```

Retain the current `console.error` for a process wait failure even when no exit
handler is installed. The host-visible log is still required for an
unobserved/background failure.

`onProcessExit` should become:

```ts
onProcessExit(
	pid: number,
	handler: (result: ProcessExitResult) => void,
): () => void {
	const entry = this._processes.get(pid);
	if (!entry) throw new Error(`Process not found: ${pid}`);
	if (entry.state === "exited") {
		handler({ ok: true, exitCode: entry.exitCode });
		return () => {};
	}
	if (entry.state === "failed") {
		handler({ ok: false, error: entry.error });
		return () => {};
	}
	entry.exitHandlers.add(handler);
	return () => entry.exitHandlers.delete(handler);
}
```

Thus registration itself throws only for an unknown/expired PID. A known failed
process is a terminal callback result, matching Rust and late success behavior.

Do not change `waitProcess`, stdout/stderr subscriptions, or the sidecar event
pump in this item. Item 69 owns per-listener isolation for shared-sidecar
stdout/stderr callbacks. Exit-handler exception isolation is not the route-loss
contract in Item 57; avoid expanding this revision unless a focused callback
test exposes a required safety regression.

### Public call sites

Update the only current repository example and test call sites found by exact
search:

- `examples/core/vm.ts:53` should branch on `result.ok` and log either the exit
  code or error;
- `packages/core/tests/spawn-flat-api.test.ts:30-35` should resolve a
  `ProcessExitResult`, assert `ok`, then inspect `exitCode`; and
- `packages/core/tests/leak-agent-os-processes.test.ts:98-100,139-175` should
  expect the success object rather than a bare number.

There are no repository callers of the public Rust `AgentOs::on_process_exit`;
the similarly named kernel process-table hook is an unrelated internal PID
cleanup callback and must not change. `website/src/content/docs/docs/core.mdx`
embeds `examples/core/vm.ts` through `CodeSnippet`, so the example edit updates
the rendered process documentation without duplicating inline code.

Do not retain an overload accepting `(exitCode: number) => void`. The protocol
and SDKs ship in lockstep with no compatibility guarantee, and an overload
would preserve the exact success-only ambiguity being removed.

## Focused before/after tests

### Rust unit tests: `crates/client/src/process.rs`

Test the extracted `subscribe_process_exit` helper with real watch channels and
oneshot observation:

1. **Protocol terminal failure mapper:** build a `wire::ProcessExitedEvent`
   whose `exit_code` is `0` but whose `error` is
   `RejectedResponse { code: "capture_failed", message: "capture failed" }`.
   Assert `process_exit_from_event` followed by `process_exit_result` returns
   `Err(ClientError::Kernel { .. })` with both exact strings. Against the parent,
   `run_spawn_events` ignores this field and publishes `Exited(0)`.
2. **Protocol terminal success mapper:** the same event with `error: None`
   remains `Ok(0)`. This guards against treating nonzero Linux exit status as a
   transport error; ordinary exit status is still a successful terminal result.
3. **Live lag failure:** subscribe while the watch contains `None`, send
   `ProcessExit::EventStreamLagged { skipped: 3 }`, and assert the callback
   receives exactly one `Err(ClientError::EventStreamLagged { skipped: 3 })`.
   Before the fix, equivalent `on_process_exit` logic logs the error and drops
   the callback.
4. **Live route closure:** send `ProcessExit::EventStreamClosed` and assert
   exactly one `Err(ClientError::EventStreamClosed { context: "process exit" })`.
5. **Unexpected watch close:** drop the sender without writing a terminal value
   and assert the same typed closed result rather than no callback.
6. **Retained failure:** seed the watch with a lag/closed/protocol-failure state
   before subscription, assert the callback runs synchronously, and assert the
   returned subscription is a no-op.
7. **Success and unsubscribe:** preserve one success case (`Ok(7)`) and one
   drop-before-terminal case proving no callback after unsubscribe.

Use bounded `tokio::time::timeout` around asynchronous observations so a
regression fails rather than hangs. Keep the existing
`process_exit_preserves_typed_event_lag` and
`closed_process_event_stream_is_an_error_not_exit_zero` tests; they cover the
shared mapper used by both wait and callback paths.

No real sidecar is required to prove callback delivery or the pure protocol
mapper. Existing Rust process E2E coverage proves genuine terminal codes reach
`wait_process`, while
`sidecar_bounds_captured_output_without_limiting_raw_streams` proves the same
wire rejection code/message already reaches finite `exec`. The new mapper unit
test closes the spawned-process-specific gap without adding a slow failure
fixture.

### TypeScript unit tests: `packages/core/tests/leak-agent-os-processes.test.ts`

Extend the existing mock `ManagedProcess.wait()` harness:

1. In the successful early/late cases, assert handlers receive
   `{ ok: true, exitCode: 0/7 }`.
2. In “failed process retains a lightweight typed failure for late waiters,”
   register an exit handler before `rejectWait(routeError)`. Assert it receives
   exactly `{ ok: false, error: routeError }` once. Before the fix it is cleared
   and never called.
3. After the failure is compacted, register a second handler and assert it is
   invoked synchronously with the exact same error object. Before the fix,
   `onProcessExit` throws `routeError` from the registration call.
4. Preserve assertions that `waitProcess` rejects with the same object and the
   map retains only `{ state: "failed", error }`.
5. Add/retain an unsubscribe case: remove a running handler, reject the wait,
   and prove the removed handler is not invoked.

These tests validate parity with Rust without manufacturing an exit code.

The rejected mock wait is also the client-level terminal-error test: it forwards
the exact error object produced by the native proxy for a wire
`ProcessExitedEvent.error`. Do not duplicate runtime-core's generated-frame
conversion test in AgentOs.

### TypeScript real success coverage

Update `packages/core/tests/spawn-flat-api.test.ts` rather than adding another
E2E. It already proves a real sidecar process exits with code 42 and that stderr
arrives before the terminal callback. Adapt it to the result union and retain
both assertions.

### Test ownership and client-to-sidecar moves

No Item 57 test should move to the sidecar:

- invoking a Rust closure or JavaScript handler is necessarily client-owned;
- bounded route lag/closure and retained subscriber behavior are transport/SDK
  concerns; and
- the optional terminal `RejectedResponse` is already produced and converted by
  sidecar/runtime-core tests; Item 57 tests only the Rust mapper and host
  callback delivery; and
- real process exit generation, output-before-terminal ordering, kill/timeout
  status, and lifecycle remain covered in existing sidecar tests.

Do not add sidecar behavior or a wire callback merely to notify host-local
listeners.

## Dependencies, risks, and non-goals

- **Item 22 is foundational:** it supplies typed lag/close outcomes and
  fail-closed process cleanup. Do not weaken that behavior.
- **Item 29 is foundational:** both early and late callbacks must use its
  bounded retained success/failure correlation.
- **Item 63 is compatible in either order:** Item 57 forwards exact errors, so
  the later `ProcessTerminalError` class flows through without changing the
  callback union. Item 63's research currently says Rust already maps spawned
  terminal errors; the audit above shows only finite `exec` does. Item 57 must
  supply that missing spawned-process mapping.
- **Item 69 remains separate:** it owns stdout/stderr listener isolation in the
  shared-sidecar pump, not terminal outcome typing.
- **Item 72 must preserve this contract:** compacting Rust terminal entries
  later must retain enough success/failure information for late
  `wait_process` and `on_process_exit` parity.
- **Breaking API:** every existing callback must branch on a result. This is
  intentional; retaining a bare-code overload would let failures disappear.
- **Exact error identity:** TypeScript must deliver the retained object; Rust
  must preserve typed fields. Do not stringify errors.
- **Exactly once:** a route failure is terminal just like exit. Never send an
  error followed by a code or vice versa.
- **Ordinary nonzero exit is not an error:** only an explicit wire
  `RejectedResponse` or route failure produces the error branch. Preserve Linux
  exit statuses such as `42` as `{ ok: true, exitCode: 42 }` / `Ok(42)`.
- **No sidecar changes:** the sidecar cannot and should not own host callbacks.
- **No invented policy:** this item does not retry, reinterpret, or convert a
  route failure into a Linux process status.

## Dedicated JJ revision and bounded paths

Implement Item 57 in one dedicated child revision. The expected diff is bounded
to:

```text
crates/client/src/agent_os.rs
crates/client/src/process.rs
crates/client/src/lib.rs
packages/core/src/runtime.ts
packages/core/src/agent-os.ts
packages/core/src/index.ts
packages/core/tests/leak-agent-os-processes.test.ts
packages/core/tests/public-api-exports.test.ts
packages/core/tests/spawn-flat-api.test.ts
examples/core/vm.ts
docs/thin-client-migration.md       # checklist/status only after validation
```

No sidecar, protocol schema, runtime-core transport, actor, or generated file
should change.

Suggested description:

```text
fix(client): surface process exit route failures
```

## Ordered implementation sequence

1. In the dedicated Item 57 `jj` child, add the failing Rust mapper/subscription
   tests and TypeScript early/late failure tests first. Record the parent
   behavior: Rust terminal-error mapping incorrectly succeeds, Rust callback
   error observation times out, and TypeScript early callbacks are never called
   while late registration throws.
2. Add Rust `ProcessExit::Failed`, the pure wire-event mapper, and the
   `process_exit_result` arm. Change unexpected watch closure to the typed
   closed-route error.
3. Add/re-export Rust `ProcessExitResult`, extract `subscribe_process_exit`, and
   change `on_process_exit` to deliver the result exactly once.
4. Add/re-export TypeScript `ProcessExitResult`; migrate `_trackProcess`,
   `spawn`, and `onProcessExit` without changing `ManagedProcess.wait()` or the
   event pump.
5. Update all three TypeScript call sites and the root public-type export test.
   The website consumes `examples/core/vm.ts` directly. Do not add a
   compatibility overload.
6. Run focused red-to-green tests, then client type/check gates and the website
   build. Only then check the tracker “after” and “complete” boxes and seal the
   one revision.

## Validation commands

The before-behavior evidence is:

- existing `process_exit_preserves_typed_event_lag` proves the failure is
  retained by the Rust mapper even though the current callback drops it;
- existing `failed process retains a lightweight typed failure for late
  waiters` proves TypeScript retains/rejects the exact error even though
  callbacks do not receive it; and
- runtime-core's `event-buffer` tests plus Rust finite-exec capture-limit E2E
  prove the optional protocol rejection already crosses the sidecar boundary.

Run that green parent evidence before adding the red assertions:

```sh
cargo test -p agentos-client --lib process_exit_preserves_typed_event_lag
pnpm --dir packages/core exec vitest run tests/leak-agent-os-processes.test.ts
pnpm --dir packages/runtime-core exec vitest run tests/event-buffer.test.ts
cargo test -p agentos-client --test process_e2e \
  sidecar_bounds_captured_output_without_limiting_raw_streams
```

Add the focused assertions described above and run them against the parent to
capture the actual red callback/mapper behavior. Then run them green:

```sh
cargo test -p agentos-client --lib process
pnpm --dir packages/core exec vitest run tests/leak-agent-os-processes.test.ts
pnpm --dir packages/core exec vitest run tests/public-api-exports.test.ts
pnpm --dir packages/core exec vitest run tests/spawn-flat-api.test.ts
```

Then run the affected client gates:

```sh
cargo fmt --all --check
cargo check -p agentos-client
pnpm --dir packages/core check-types
pnpm --dir website build
```

Item 57 is complete only when both clients deliver success or typed failure
exactly once, early and late subscribers behave consistently, no failure is
represented by a fabricated exit code, all repository call sites use the new
outcome, and the tracker checklist is updated in this dedicated revision.
