# Item 37 research — Rust cron callback results

Status: implementation-ready research only. This note does not change the Item 37
tracker row or implementation. It was refreshed against Item 36's in-progress
working copy (`lqprmlyn`). Item 37 should be implemented only after Item 36 is
sealed, in its own stacked `jj` revision.

## Priority and confidence

- Priority: **P1**. A fallible Rust host callback has no supported failure value,
  so the client acknowledges the run as successful and the sidecar emits
  `CronEventKind::Complete`.
- Fix confidence: **high**. The wire request, native and browser dispatchers, and
  shared sidecar scheduler already implement the correct error path. Only the
  Rust host callback boundary cannot currently populate it.
- Scope confidence: **high**. The complete production change is confined to
  `crates/client/src/cron.rs` plus the public re-export in
  `crates/client/src/lib.rs`; the remaining edits are tests and call-site updates.

## Original issue and exact current path

The callback closure is one of the few resources that legitimately remains in a
thin client: an in-process Rust closure cannot cross the protocol. The defect is
not that Rust runs the callback; it is that Rust discards its outcome.

1. `crates/client/src/cron.rs:34-50`, `CronAction::Callback`, requires
   `BoxFuture<'static, ()>`.
2. That unit-returning type is copied into `CallbackRoute.callback` at lines
   201-205 and `CronManager::allocate_callback` at lines 309-327.
3. `CronManager::callback_for_run` at lines 354-365 returns the same unit future
   and increments the route's in-flight count.
4. `CronManager::execute_run` at lines 557-600 already sends
   `action_result.err()` as `CompleteCronRunRequest.error`.
5. `run_host_action` at lines 614-631 awaits the callback at line 627, discards
   the output, and manufactures `Ok(())` at line 628. A caller therefore cannot
   provide the error that `execute_run` already knows how to forward.
6. The placeholder returned when a sidecar-owned job names a callback route that
   is unavailable on this host, `CronManager::callback_action` at lines 375-395,
   only logs at lines 387-390 and also returns unit. If invoked, that placeholder
   is likewise recorded as a successful run.

This produces a real behavioral mismatch with TypeScript:
`packages/core/src/cron/cron-manager.ts:204-243`, `CronManager.executeRun`, catches
a thrown or rejected callback, derives its message at line 222, and sends it to
`completeCronRun` at lines 236-241.

The phrase “durable failure recorded as success” means a callback belonging to a
sidecar-owned/durable cron job is emitted as `Complete` instead of `Error`. The
current cron snapshot does **not** store a last-success/last-error outcome: it
stores scheduling and run-count state. Item 37 must not invent a new persisted
outcome field.

## Why this does not require sidecar or protocol changes

- `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:489-492` already
  defines `CompleteCronRunRequest { runId, error: optional<str> }`.
- `crates/native-sidecar-core/src/cron.rs:369-397`, `CronScheduler::complete`,
  already completes/removes the active run, advances queued work, and feeds the
  optional error to the completion event.
- `crates/native-sidecar-core/src/cron.rs:691-703`, `completion_event`, already
  selects `CronEventKind::Error` when `error` is present and truncates the host
  text at the sidecar-owned `MAX_CRON_ERROR_BYTES` limit.
- Native dispatch forwards the request unchanged through
  `crates/native-sidecar/src/service.rs:1861-1876`, `complete_cron_run`.
- Browser dispatch does the same through
  `crates/native-sidecar-browser/src/wire_dispatch.rs:570-590`,
  `complete_cron_run`.
- The shared scheduler already has the direct classification regression
  `completion_event_records_sidecar_duration_and_error` at
  `crates/native-sidecar-core/src/cron.rs:1172-1204`.

Do not add a result union, a client-selected error cap, or another scheduler
state machine. The Rust client should supply the already-supported optional
error string; the sidecar continues to own classification, limits, run state,
and lifecycle events.

## Exact replacement

### 1. Define one public Rust callback boundary

In `crates/client/src/cron.rs`, immediately before `CronAction`, add:

```rust
/// Result returned by a host cron callback and forwarded to the sidecar.
pub type CronCallbackResult = Result<(), String>;

/// Host callback retained by the client because closures cannot cross the wire.
pub type CronCallback = Arc<
    dyn Fn() -> futures::future::BoxFuture<'static, CronCallbackResult>
        + Send
        + Sync,
>;
```

Then replace the four repeated callback signatures with `CronCallback`:

- `CronAction::Callback { callback: CronCallback }`
- `CallbackRoute { callback: CronCallback, ... }`
- `CronManager::allocate_callback(&self, callback: CronCallback)`
- `CronManager::callback_for_run(...) -> Result<CronCallback, String>`

`Result<(), String>` is deliberately wire-shaped. It lets the host choose the
message, matches the existing `CronAlarmHandler` error boundary at
`crates/client/src/cron.rs:126-130`, and avoids introducing `anyhow`, a boxed
error policy, or a `ClientError::Sidecar` display prefix into user callback text.

Re-export both aliases from `crates/client/src/lib.rs:85-88` with the existing
cron types:

```rust
pub use cron::{
    CronAction, CronAlarmHandler, CronAlarmUpdate, CronCallback,
    CronCallbackResult, CronEvent, CronJobHandle, CronJobInfo, CronJobOptions,
    CronManager, CronOverlap,
};
```

### 2. Preserve the exact error string through the existing request

Change `callback_for_run` to return `Result<CronCallback, String>` and preserve
its current route lookup plus `active_runs += 1` behavior:

```rust
fn callback_for_run(&self, callback_id: &str) -> Result<CronCallback, String> {
    let mut registry = self.callbacks.lock();
    let route = registry
        .routes
        .get_mut(callback_id)
        .ok_or_else(|| format!("cron callback route not found: {callback_id}"))?;
    route.active_runs += 1;
    Ok(route.callback.clone())
}
```

Change `run_host_action` to return `CronCallbackResult` and return the callback
future's result directly:

```rust
async fn run_host_action(
    manager: &Arc<CronManager>,
    action: WireCronAction,
) -> CronCallbackResult {
    match action {
        WireCronAction::Session { .. } => Err(String::from(
            "sidecar returned non-host cron action to client: session",
        )),
        WireCronAction::Exec { .. } => Err(String::from(
            "sidecar returned non-host cron action to client: exec",
        )),
        WireCronAction::Callback { callback_id } => {
            let callback = manager.callback_for_run(&callback_id)?;
            callback().await
        }
    }
}
```

At `execute_run` lines 562-570, map JSON decode failure directly to `String`
instead of `ClientError`, then leave line 589 semantically as a direct
`action_result.err()` assignment:

```rust
let action = serde_json::from_str::<WireCronAction>(&run.action)
    .map_err(|error| format!("invalid cron action: {error}"));
// ...
error: action_result.err(),
```

Do not use `.map(ClientError::Sidecar)` or `.map(|error| error.to_string())` for
the callback result. `ClientError::Sidecar` displays as `sidecar error: ...`,
which would decorate Rust's callback message while TypeScript forwards the
message exactly.

Retain the existing `complete_callback_run` call before the transport request.
It is host-route lifecycle cleanup, not run classification, and must happen for
both `Ok` and `Err` callback results.

### 3. Make a missing local callback route fail when invoked

In `CronManager::callback_action` at lines 375-395, replace the logging-only
placeholder future with a typed failure:

```rust
Arc::new(|| {
    Box::pin(async {
        Err(String::from(
            "cron callback route is unavailable on this host",
        ))
    })
})
```

The placeholder is used only to represent sidecar job information through the
public `CronAction` type. Listing a job must remain side-effect-free. If a caller
explicitly invokes that returned closure, the result should describe failure;
it should not log and pretend success. No callback ID needs to be exposed in the
public message.

### 4. Update all current Rust callback call sites

A repository-wide search currently finds exactly three constructing call sites:

- `crates/client/tests/cron_e2e.rs:35-41`: notify, then return `Ok(())`.
- `crates/client/tests/cron_e2e.rs:85-87`: use
  `Box::pin(async { Ok(()) })`.
- `crates/client/tests/cron_grammar_e2e.rs:14-18`: use
  `Box::pin(async { Ok(()) })`.

The actor plugin schedules serializable exec/session actions and does not
construct `CronAction::Callback`; no actor production call site changes.

Do not catch panics in Item 37. `Err(String)` is the supported failure result. A
panic is a host task failure and needs a separate, explicit unwind/lifecycle
design if it is to become recoverable.

## Validation checklist

### Before behavior

Use the Item 36 parent for both pieces of evidence:

1. Add the after-regression callback returning
   `Err("rust callback failed".to_string())`. `cargo test -p agentos-client
   --test cron_e2e --no-run` fails with the callback future output mismatch
   (`expected (), found Result`). This proves the public API cannot express a
   callback failure.
2. For a runnable baseline demonstration, temporarily make the old unit-returning
   callback await a `Result::Err`, assert/record that host failure locally, then
   return `()`. The existing real-sidecar event loop observes
   `CronEvent::Complete` and no `CronEvent::Error`. This proves the forced discard
   is acknowledged as success. Remove this temporary old-shape case after
   recording the tracker evidence.

### After: fast Rust forwarding regression

Add a private unit test next to the existing tests in
`crates/client/src/cron.rs:959-1045`:

- create `Arc<CronManager>`;
- allocate a callback returning `Err("rust callback failed".to_string())`;
- run the corresponding `WireCronAction::Callback` through `run_host_action`;
- assert the result is exactly `Err("rust callback failed")`;
- call `complete_callback_run` and assert the unscheduled route is released.

This directly guards the code that formerly manufactured `Ok(())` without a
real-time cron delay.

### After: real Rust sidecar classification

Add `failed_cron_callback_is_recorded_as_error` to
`crates/client/tests/cron_e2e.rs`:

1. Subscribe with `os.cron_events()` before scheduling.
2. Schedule a uniquely named one-shot callback about one second in the future;
   its future returns `Err("rust callback failed".to_string())`.
3. Within one bounded eight-second wait, consume events until the exact job has
   emitted `CronEvent::Fire` and then `CronEvent::Error` with the exact message.
4. Fail immediately if that job emits `CronEvent::Complete`.
5. Query `list_cron_jobs()` and assert that job has `run_count == 1` and
   `running == false`. This proves the sidecar terminalized the run instead of
   orphaning it.
6. Cancel the job and shut the VM down normally.

Do not assert that `export_cron_state` restores the error text: the current
snapshot intentionally has no last-outcome field. The error event and the
authoritative terminal run state are the relevant behavior.

### After: TypeScript parity/reference coverage

No TypeScript production change is required, but add the missing regression to
`packages/core/tests/cron-manager.test.ts` after the existing callback routing
test at lines 92-129:

- schedule a callback that rejects with
  `Error("typescript callback failed")`;
- dispatch one callback run;
- assert `completeCronRun(session, sidecarVm, "run-failed",
  "typescript callback failed")`.

Add the matching real-sidecar case beside
`packages/core/tests/cron-integration.test.ts:39-61`:

- subscribe before scheduling;
- reject the one-shot callback;
- require `cron:fire` followed by `cron:error` for the exact job and message;
- reject any `cron:complete` for that job;
- assert `runCount === 1` and `running === false`, then cancel.

This makes the tracker claim (“Rust and TypeScript record the same failed run”)
executable without moving any TypeScript policy.

### Preserve the alarm/wake hook

Do not change these symbols:

- `CronAlarmHandler`, `CronManager::apply_alarm`, or
  `AgentOs::wake_cron_generation` in `crates/client/src/cron.rs`.
- `crates/agentos-actor-plugin/src/vm.rs:77-97`, which turns the sidecar's
  absolute timestamp and opaque generation into one Rivet
  `__agentos_cron_wake` `schedule_at` action.
- `crates/agentos-actor-plugin/src/actions/mod.rs:856-867`, which returns that
  generation to the sidecar and persists the resulting opaque scheduler state.

Run the focused actor persistence test to guard the timestamp/action/generation
bridge. The cold-boot test is useful when a sidecar binary is supplied, but Item
40 separately owns making that integration mandatory in CI.

## Focused commands

```sh
cargo build -p agentos-sidecar

cargo test -p agentos-client --lib cron::tests -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test cron_e2e -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test cron_grammar_e2e -- --nocapture

AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/core exec vitest run \
    tests/cron-manager.test.ts tests/cron-integration.test.ts \
    --reporter=verbose

cargo test -p agentos-actor-plugin \
  persistence_e2e::persistence_stores_cron_state_as_an_opaque_value \
  -- --exact --nocapture

cargo check -p agentos-client -p agentos-actor-plugin
pnpm --dir packages/core check-types
cargo fmt --all -- --check
git diff --check
```

The final Item 37 stack gate should also run the repository-required
`cargo check --workspace`, `pnpm build`, and `pnpm check-types` commands.

## Dependencies, boundaries, and sealing risks

- **Stack dependency:** Item 36 must be sealed first. Item 37 then gets one new
  stacked `jj` revision and only its own implementation/tests/tracker edits.
- **Item 40 is independent:** preserving actor alarm behavior belongs in Item 37;
  making the cold-boot test impossible to skip in CI remains Item 40.
- **Intentional Rust API break:** every successful Rust callback must now return
  `Ok(())`. Search for `CronAction::Callback` and `CronCallback` before sealing.
- **No client policy:** the client forwards the caller's string. The shared
  sidecar remains responsible for error truncation and event classification.
- **No sidecar callback execution:** native/browser sidecars cannot access a host
  closure. Moving it would require a different serializable action, not a cleanup
  of this API.
- **Bounded tests:** subscribe before scheduling and use a single outer deadline;
  callback completion is asynchronous and a one-shot event can otherwise be
  missed.
