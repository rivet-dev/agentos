# Item 36 browser implementation audit

Status: supplemental implementation checklist, initially audited after Item 35
at `11bc452b` and reconciled with the in-progress Item 36 shared-core changes at
`87c8a1ec`. This document changes no production code or migration status.

## Seal blocker

The browser half of Item 36 is not retry-safe yet. A failed cleanup crosses four
ownership layers, and every layer currently either retains an active-looking
route or destroys the handle needed by the next attempt:

1. `AcpCore` owns the process/session correlation.
2. `BrowserAcpExtension` owns the core process id to browser execution/context
   route.
3. `BrowserSidecar` owns the kernel pid and worker handle.
4. `BrowserWireDispatcher` owns the connection/session/VM close transaction.

The required retry flow is inside-out:

```text
wire close retry
  -> extension owner-cleanup retry
    -> ACP route-cleanup retry
      -> executor phase retry
        -> kernel reap / worker termination / events complete
      -> context release completes
    -> core cleanup tombstone is removed
  -> VM and session extension phases complete
-> successful close is recorded in terminal history
```

No failed state in that chain may remain usable by stdin, signal, poll, prompt,
extension, or ordinary VM requests. Retaining a cleanup handle is not retaining
a route.

## Exact current defects

### `crates/agentos-sidecar-browser/src/acp_host.rs`

- `BrowserAcpHost::execution_id` treats every entry in `executions` as active.
  `abort_agent` keeps that same entry when `abort_execution` or
  `release_context` fails. The retained retry handle therefore still has the
  representation of a routable process.
- The real `BrowserSidecar::abort_execution` removes `ExecutionState` inside
  `release_execution` before returning a worker/kernel cleanup error. On the
  second `BrowserAcpHost::abort_agent` call, the sidecar reports the missing
  execution as already cleaned; the ACP route is removed without reattempting
  the failed worker termination.
- `finalize_session_cleanup` has the same false-success retry: its first
  `release_execution` call can destroy the executor handle and fail; its second
  call sees no execution and proceeds to remove the ACP route.
- `abort_agent` attempts context cleanup after execution cleanup, but has no
  per-phase state. A successful context release is repeated after an execution
  failure, and a successful execution cleanup is repeated after a context
  failure.
- The in-progress shared-core patch now maps `BrowserSidecarError::Cleanup` to
  `AcpCoreError::Cleanup` and calls `finalize_session_cleanup` before removing
  the core session. The browser host still needs the non-routable phase record
  described below; the new hook alone cannot recover the executor handle that
  `BrowserSidecar::release_execution` destroys.

### `crates/agentos-sidecar-browser/src/lib.rs`

- `BrowserAcpExtension::executions` is one untyped route map. Diagnostics cannot
  distinguish active routes from retained cleanup handles.
- `dispose_owners` calls `AcpCore::dispose_owner`, stringifies every error, and
  returns `BrowserSidecarError::InvalidState`. Cleanup identity and the stable
  `cleanup_failed` code are lost.
- `on_session_disposed` derives owners only from the route map. Once active and
  cleanup routes are split, it must include both or use an explicit owner
  registry. `on_vm_disposed` already receives the exact owner and should remain
  the primary close path.
- The current test
  `browser_acp_host_retains_route_until_abort_cleanup_can_be_retried` uses a mock
  whose failed abort leaves its execution available. It does not model the real
  sidecar's early `ExecutionState` removal and therefore misses the false-success
  retry.

### `crates/native-sidecar-browser/src/service.rs`

- `release_execution` removes `ExecutionState`, `active_executions`, and signal
  state before kernel reap and `terminate_worker`. Any returned cleanup error is
  unretryable.
- Kernel reap and worker termination are aggregated only when both fail. A
  single failure returns its raw child type, so the error shape changes with
  cardinality.
- Structured release and lifecycle-event failures are logged after cleanup and
  cannot be retried. If these remain best-effort diagnostics, the code must say
  so explicitly and keep the existing host-visible log. If they are part of the
  cleanup contract, they need progress bits like the resource phases. For Item
  36 parity, make them tracked phases.
- `abort_execution` combines kill and release failures into
  `InvalidState(String)` rather than `BrowserSidecarError::Cleanup`.
- `dispose_vm` removes `VmState` before execution cleanup. Consequently
  `reap_execution_kernel_process` treats the missing VM as success, worker
  failures lose their handles, and a retry gets `unknown browser sidecar VM`.
- `dispose_extension_session_state` and `dispose_extension_vm_state` run every
  extension but retain only `first_error`. They also have no namespace progress,
  so a retry would repeat callbacks that already succeeded.
- Existing tests named `dispose_vm_drains_maps_even_when_worker_termination_fails`
  and `release_execution_preserves_both_cleanup_errors_after_draining_maps`
  encode handle destruction as success criteria. They must be replaced, not
  preserved alongside retry state.

### `crates/native-sidecar-browser/src/wire_dispatch.rs`

- `close_session` retains only `first_error`, purges every VM route, removes the
  session, releases its admission slot, and stores the failed attempt in terminal
  history. A retry only replays the old string; no cleanup runs.
- The existing test
  `browser_failed_close_replays_the_terminal_failure_and_releases_admission`
  explicitly locks in that incorrect behavior.
- `dispose_vm` also purges ownership after failure and formats the extension/VM
  pair ad hoc. It needs the same per-VM cleanup driver as session close.
- `purge_vm_state` currently mutates every session's `vm_ids`. Cleanup ownership
  must be recorded before this route purge, and route purge must not delete the
  cleanup record.
- `session_close_outcomes` must contain only final successful outcomes (or a
  truly terminal result with no remaining handle). A transient cleanup failure
  is not terminal history.

## Patch checklist

Implement in this order so each outer retry has a real inner retry to invoke.

### 1. Make browser errors typed and deterministic

- [x] In `agentos-sidecar-core`, add/use `AcpCoreError::Cleanup { context,
  errors }` with code `cleanup_failed`. Always use it for one or more cleanup
  children; do not return the raw child for a one-error aggregate. The
  in-progress shared-core patch supplies the variant and code.
- [x] In `acp_host.rs::map_err`, map `BrowserSidecarError::Cleanup` recursively
  to `AcpCoreError::Cleanup`. Keep `InvalidState`, `Conflict`, and
  `LimitExceeded` mappings unchanged. This browser mapping is already present
  in the in-progress revision.
- [ ] In `lib.rs::to_browser_error`, preserve `AcpCoreError::Cleanup` as
  `BrowserSidecarError::Cleanup`; do not turn it into `InvalidState`.
- [ ] Add one helper in `native-sidecar-browser/src/service.rs`, for example
  `cleanup_result(context, errors)`, that returns `Ok(())` for an empty vector
  and `BrowserSidecarError::Cleanup` for every non-empty vector.
- [ ] Attach operation identity before pushing each child: execution id, VM id,
  extension namespace, and phase. Preserve operation order; never sort rendered
  error strings.

Stable browser phase order:

1. abort signal, when applicable;
2. kernel reap;
3. worker termination;
4. execution structured event;
5. execution lifecycle event;
6. context release;
7. extension namespaces in `BTreeMap` order;
8. VM ids in `BTreeSet` order;
9. session-level extension namespaces.

### 2. Replace active executor entries with lifecycle records

- [ ] In `service.rs`, replace
  `BTreeMap<String, ExecutionState>` with a lifecycle value:

```rust
enum ExecutionLifecycle {
    Active(ExecutionState),
    Cleaning(ExecutionCleanupState),
}

struct ExecutionCleanupState {
    execution: ExecutionState,
    event_name: &'static str,
    abort_signal_complete: bool,
    kernel_reaped: bool,
    worker_terminated: bool,
    structured_event_emitted: bool,
    lifecycle_event_emitted: bool,
}
```

- [ ] Add `begin_execution_cleanup`. It must transition `Active -> Cleaning`,
  remove the id from `VmState::active_executions` and `signal_states`, and only
  then perform a fallible operation. Calling it on `Cleaning` returns the
  existing progress record; calling it after full removal is idempotent success.
- [ ] Make `ensure_execution`, `ensure_execution_state`, stdin, signal, kill,
  poll, and event routing match only `Active`. A cleaning id must return a typed
  non-active/invalid-state error and must not reach the bridge or kernel.
- [ ] Add `drive_execution_cleanup(execution_id)`. Attempt every incomplete
  independent phase, set a bit only after success, and remove the lifecycle
  entry only after all required bits are set.
- [ ] `abort_execution` must transition first, then drive abort signal and
  release phases. If reap/termination proves the execution gone, mark a failed
  abort signal superseded rather than signalling a dead worker forever.
- [ ] `release_execution` must call the same driver without an abort-signal
  requirement. A retry must never repeat a successful kernel reap, worker
  termination, or event phase.
- [ ] Late bridge events for a cleaning id must not reactivate it. Drop/log
  terminal output for that id or route only the cleanup-relevant terminal fact.

#### Bound and warning

- [ ] Add
  `BrowserSidecarConfig::max_pending_execution_cleanups_per_vm` with a finite
  default and a public default constant. Every active execution reserves one
  possible cleanup slot, so admission in `start_execution_with_options` checks
  `active + cleaning` before creating a kernel process or worker.
- [ ] On exhaustion return `BrowserSidecarError::LimitExceeded` with limit
  `max_pending_execution_cleanups_per_vm` and `how_to_raise` naming the exact
  config field.
- [ ] Warn once at the existing 80% threshold and clear the warned bit after
  usage drops below it. Store the warned bit per VM.
- [ ] Expose test-only counts for active executions and cleaning executions.
  Do not count cleaning entries in `active_worker_count`.

This reservation rule prevents permanent termination failures plus replacement
workers from growing the lifecycle map without bound.

### 3. Make VM disposal a non-routable lifecycle

- [ ] Add `VmLifecycle::{Active, Cleaning}` to `VmState` rather than moving a
  failed VM into an unbounded second map. `ensure_vm` and every ordinary VM API
  must accept only `Active`; cleanup helpers may access `Cleaning` directly.
- [ ] On the first `dispose_vm`, mark the VM cleaning before fallible work,
  remove its contexts from ordinary routing, and transition every active
  execution to `ExecutionLifecycle::Cleaning`.
- [ ] Drive every execution cleanup in execution-id order even after an earlier
  failure. Retain the `VmState` because it owns the kernel needed for retry.
- [ ] Track the terminal VM lifecycle event as a VM cleanup phase. Remove
  `VmState` only when all execution records and the VM event phase are complete.
- [ ] A second `dispose_vm` on a cleaning VM resumes incomplete phases. Ordinary
  VM/execution requests during that interval must fail as non-active.
- [ ] Split diagnostics into active VM count and pending VM cleanup count.

Do not retain a VM in an active map merely to keep its kernel. The lifecycle bit
is the enforcement check that makes the retained kernel a cleanup handle only.

### 4. Split active ACP routes from cleanup routes

- [ ] In `agentos-sidecar-browser/src/lib.rs`, replace the raw executions map
  with a store whose values are explicit lifecycles:

```rust
enum BrowserAcpRoute {
    Active(BrowserAcpExecution),
    Cleaning(BrowserAcpRouteCleanup),
}

enum BrowserAcpCleanupKind {
    Abort,
    FinalizeSession,
}

struct BrowserAcpRouteCleanup {
    route: BrowserAcpExecution,
    kind: BrowserAcpCleanupKind,
    execution_complete: bool,
    context_complete: bool,
}
```

- [ ] `BrowserAcpHost::execution_id` must match only `Active` and exact owner.
  This immediately blocks stdin, kill, poll, and protocol routing for cleanup
  entries.
- [ ] `abort_agent` and `finalize_session_cleanup` must atomically transition
  the route before invoking `BrowserExtensionContext`. On retry they resume the
  stored kind and incomplete bits. The shared core already marks the session
  closed before retrying finalization, so it will not repeat signal/wait phases.
- [ ] Attempt execution and context phases in deterministic order and retain all
  errors. Mark each successful phase independently. Remove the route only after
  both are complete.
- [ ] Keep owner identity in cleaning routes so another connection/session/VM
  cannot retry or observe them.
- [ ] Apply the same finite reservation used by the executor: active plus
  cleaning ACP routes for one VM may not exceed
  `max_pending_execution_cleanups_per_vm`. Pass the configured value into
  `BrowserAcpExtension` from `browser_sidecar` and the WASM constructor.
- [ ] Extend `BrowserAcpResourceCounts` with `cleanup_process_routes` (and the
  shared-core cleanup-tombstone count once added). `process_routes` must mean
  active routes only.
- [ ] Update `on_session_disposed` owner discovery to include cleaning routes;
  keep `on_vm_disposed`'s explicit owner as the authoritative path.
- [ ] Change `dispose_owners` to `BrowserSidecarError::Cleanup`, retaining owner
  and process identity. It must cooperate with the shared core's owner cleanup
  tombstones rather than calling `take_owner_state` a second time and declaring
  success.

### 5. Track extension namespace progress

- [ ] Add `BrowserSidecar::extension_namespaces()` returning the deterministic
  registered namespace set.
- [ ] Change `dispose_extension_vm_state` and
  `dispose_extension_session_state` to accept a mutable set of pending
  namespaces, or add a small exported `BrowserExtensionCleanupProgress` value.
- [ ] Snapshot namespaces when cleanup ownership transitions. Invoke every
  pending namespace in `BTreeMap` order; remove a namespace from progress only
  after its callback succeeds.
- [ ] Aggregate every failed namespace with `BrowserSidecarError::Cleanup`.
  Successful sibling callbacks must not run again on retry.
- [ ] Reinsert each temporarily removed extension in the registry before
  handling its result, exactly as the current code does.

This progress belongs to the cleanup transaction, not to the extension object:
the wire owner decides when a VM/session is no longer routable, while each
extension owns only its callback implementation.

### 6. Keep wire close ownership until cleanup finishes

- [ ] Replace `BrowserSessionState { connection_id, vm_ids }` with an explicit
  lifecycle:

```rust
struct BrowserSessionState {
    connection_id: String,
    lifecycle: BrowserSessionLifecycle,
}

enum BrowserSessionLifecycle {
    Active {
        vm_ids: BTreeSet<String>,
        vm_cleanups: BTreeMap<String, BrowserVmCleanupProgress>,
    },
    Closing {
        vm_cleanups: BTreeMap<String, BrowserVmCleanupProgress>,
        pending_session_extensions: BTreeSet<String>,
    },
}

struct BrowserVmCleanupProgress {
    pending_extensions: BTreeSet<String>,
    vm_complete: bool,
}
```

- [ ] On the first `close_session`, validate exact connection ownership, move
  every active VM into cleanup progress, set `Closing`, and purge ordinary VM,
  process, cron, capture, and pending-event routes before fallible cleanup.
- [ ] Keep the session id in `BrowserConnectionState::sessions` while closing.
  Therefore `max_sessions_per_connection` bounds active plus closing sessions
  and a permanent cleanup failure cannot release admission for unlimited new
  sessions.
- [ ] Make `owned_vm_id`, `create_vm`, extension routing, cron routing, and every
  ordinary operation require `BrowserSessionLifecycle::Active` plus an active
  VM id. A closing session is cleanup-only.
- [ ] Add one `drive_vm_cleanup` used by explicit `dispose_vm` and session close.
  It resumes pending extension namespaces and `BrowserSidecar::dispose_vm`, marks
  successful phases, and retains all failed phases.
- [ ] Session close must drive all VM records even after failures, then drive all
  pending session-extension namespaces. Return one deterministic aggregate for
  the current attempt.
- [ ] On failure, leave the session `Closing`, retain its connection ownership,
  and do not write `session_close_outcomes`.
- [ ] On full success, remove the session and connection admission entry, then
  record only the successful terminal outcome in bounded close history.
- [ ] A close retry by the same connection resumes incomplete phases. A retry by
  another connection remains `ownership_mismatch`. A third close after success
  replays the successful terminal result without rerunning cleanup.
- [ ] Refactor `purge_vm_state` into route purge plus lifecycle progress update;
  it must never erase `BrowserVmCleanupProgress`.
- [ ] Use the same typed aggregate helper in explicit `dispose_vm`; remove the
  ad-hoc two-string formatting.
- [ ] Add diagnostics for active sessions, closing sessions, active VMs, and VM
  cleanup records. All must return to zero after a successful retry.

## Exact test checklist

### `crates/native-sidecar-browser/src/service.rs`

- [ ] Replace
  `release_execution_terminates_worker_after_kernel_cleanup_failure` with
  `release_execution_makes_route_non_routable_and_retries_only_kernel_cleanup`.
  First call: active count zero, cleanup count one, worker termination succeeds
  once, kernel failure is returned. Ordinary execution APIs reject the id.
  Second call: only kernel reap repeats; cleanup count becomes zero.
- [ ] Replace
  `release_execution_preserves_both_cleanup_errors_after_draining_maps` with
  `release_execution_retains_both_failed_phases_in_order_until_retry`.
  Inject one-shot kernel and worker failures, assert `Cleanup` children are
  kernel then worker, then assert retry invokes both exactly once more and
  removes the record.
- [ ] Replace `dispose_vm_drains_maps_even_when_worker_termination_fails` with
  `dispose_vm_retains_non_routable_kernel_and_worker_cleanup_until_retry`.
  Assert active VM/context/execution counts are zero after the first failure,
  pending VM/execution cleanup counts are one, normal VM calls fail, and retry
  removes both without repeating completed phases.
- [ ] Add `execution_cleanup_reservation_blocks_replacement_growth`. Configure a
  capacity of one, leave one permanent cleanup record, assert a new start returns
  `LimitExceeded` naming the field/how-to-raise, clear the failure, retry cleanup,
  and assert admission succeeds.
- [ ] Add `dispose_extension_cleanup_retries_only_failed_namespaces`. Register
  two successful extensions around one one-shot failure; assert deterministic
  first-attempt aggregation and call counts `1, 2, 1` after retry.
- [ ] Add an event-phase regression if events are tracked: resource phases must
  not repeat after a one-shot structured/lifecycle emission failure.

### `crates/agentos-sidecar-browser/src/lib.rs`

- [ ] Replace
  `browser_acp_host_retains_route_until_abort_cleanup_can_be_retried` with
  `browser_acp_host_moves_failed_abort_to_non_routable_cleanup_route`. Use a mock
  that removes its active execution on the first failing abort, like the real
  sidecar. Assert active route count zero, cleanup route count one, ordinary host
  lookup fails, and retry invokes the retained cleanup handle rather than
  manufacturing success from absence.
- [ ] Add `browser_orderly_close_retries_release_and_context_phases_once`. Fail
  executor release once and context release once on separate attempts. Assert
  the core session remains authoritative, the ACP route is cleanup-only, each
  successful phase is called once, and the final retry clears core/route cleanup
  counts.
- [ ] Add `browser_dispose_owners_aggregates_every_owner_in_order`. Two owners
  fail once with distinct sentinels; assert typed `Cleanup`, deterministic owner
  order, exact-owner retry, and zero residual diagnostics.
- [ ] Retain `orderly_close_churn_releases_every_browser_route_without_double_abort`
  and extend it to assert zero cleanup routes/tombstones after every iteration.

### `crates/native-sidecar-browser/tests/wire_dispatch.rs`

- [ ] Replace
  `browser_failed_close_replays_the_terminal_failure_and_releases_admission`
  with
  `browser_failed_close_retains_non_routable_ownership_and_retries_cleanup`.
  With `max_sessions_per_connection = 1`, first close fails once, ordinary
  session/VM requests are rejected, and opening a replacement session remains
  limit-exceeded. The retry reruns cleanup and succeeds; only then may another
  session open. A third close replays success.
- [ ] Add `browser_close_retries_only_failed_extension_namespaces`. Use three
  extensions with the middle one failing once and assert callback totals
  `1, 2, 1`, not `2, 2, 2`.
- [ ] Add `browser_close_aggregates_vm_and_session_failures_in_stable_order`.
  Inject VM-extension, worker, and session-extension sentinels and assert all are
  present once in documented phase/VM/namespace order.
- [ ] Add `browser_close_retry_preserves_exact_connection_ownership`. While a
  close is pending, another authenticated connection gets
  `close_session_ownership_mismatch`; the owner can retry successfully.
- [ ] Add `browser_failed_close_is_not_terminal_history`. Assert failed attempts
  do not consume close-history capacity; only final success is retained.
- [ ] Add `browser_dispose_vm_uses_the_same_retryable_phase_driver`. After a
  one-shot extension or worker failure, the VM is non-routable, the session stays
  active for sibling VMs, retry completes only unfinished phases, and all VM
  ownership records reach zero.

### Wrapper parity

- [ ] In `crates/agentos-sidecar/tests/acp_wrapper_conformance.rs`, add a browser
  case for one-shot worker cleanup failure -> same-owner close retry -> success.
  Assert zero shared-core sessions, pending interactions, active ACP routes,
  cleanup ACP routes, active executions, execution cleanups, and closing wire
  sessions.
- [ ] Run that case beside the native Item 36 retry case so both wrappers expose
  `cleanup_failed` on the first attempt and the same final closed result.

## Focused validation

```sh
cargo test -p agentos-native-sidecar-browser --lib -- --nocapture
cargo test -p agentos-native-sidecar-browser --test wire_dispatch -- --nocapture
cargo test -p agentos-sidecar-browser --lib -- --nocapture
cargo test -p agentos-sidecar --test acp_wrapper_conformance -- --nocapture
cargo check -p agentos-sidecar-browser --target wasm32-unknown-unknown
cargo check --workspace
cargo fmt --all -- --check
git diff --check
```

Run wrapper conformance at least twice. The second run is useful for detecting
late worker events accidentally delivered to an execution already in cleanup.

## Seal conditions

The browser portion of Item 36 is sealable only when:

- active routing becomes unreachable before the first fallible cleanup action;
- every retry invokes a retained handle and only incomplete phases;
- active plus cleanup records are bounded at admission and warn near capacity;
- failed wire close ownership remains exact and consumes its existing session
  admission slot;
- failed attempts are absent from terminal close history;
- typed cleanup aggregates retain every child in deterministic order; and
- all active and cleanup diagnostics reach zero after retry success.
