# Item 36 native supplement: exact cleanup patch and test checklist

Status: research only, against working-copy revision `579db25fcd2f` after Item
35. This note does not change production code or `docs/thin-client-migration.md`.

## Native conclusion

Keep the current shared-core boundary. The current tree has already fixed the
largest ordering problem described in `item-36.md`: native terminal and
extension-resource cleanup now runs from
`AcpHost::release_agent_route`/`NativeCoreCommand::ReleaseAgentRoute`, and
`AcpCore::close_session` removes its authoritative session only after that hook
succeeds. Do **not** add a second finalization hook or restore a post-dispatch
cleanup block.

Three native problems remain:

1. Cleanup loops use `first_error`, `?`, or an `Execution(String)` join, so a
   caller sees only a prefix of the failures. Native cleanup needs one stable,
   typed, operation-ordered aggregate.
2. ACP route finalization does not retain per-phase progress, and
   `ExtensionHost::dispose_session_resources` removes its only retry record
   before fallible cleanup. A failed close can therefore repeat completed work,
   skip later work, or turn the retry into an apparent no-op.
3. stdio runs disconnect cleanup only after a clean event-loop break and then
   discards `remove_connection` errors. Read, dispatch, event-pump, and writer
   failures bypass cleanup entirely; normal EOF hides cleanup failures.

This remains sidecar-only work. No TypeScript or Rust SDK path should change.

## What is already correct in the current tree

- `crates/agentos-sidecar-core/src/engine.rs::AcpCore::close_session` calls
  `host.release_agent_route` before `remove_session` and has
  `close_session_retains_authoritative_state_until_cleanup_can_be_retried`.
- `crates/agentos-sidecar/src/acp_extension.rs::release_native_agent_route`
  retains `core_processes[route_key]` until its current cleanup sequence
  succeeds.
- `cleanup_session_terminals` snapshots terminal ids from a `BTreeMap`, visits
  them deterministically, attempts both kill and output-buffer cleanup, removes
  only successful terminal records, and retains failed terminal records.
- Native service session ids, VM ids, process ids, extension namespaces, and
  connection ids already come from `BTreeSet`/`BTreeMap` iteration. Preserve
  those identity orders; do not sort rendered error strings.
- `dispose_vm_internal` already detaches the VM and unconditionally calls
  `reclaim_vm_tracking`, so post-detach failures do not leave a routable VM.

## Exact remaining defects

| Priority | File / symbol | Current behavior | Required result |
| --- | --- | --- | --- |
| P0 | `acp_extension.rs::release_native_agent_route` (currently 1223-1239) | `stop_buffering_process_output?`, `cleanup_session_terminals?`, and `dispose_session_resources_wire?` are one untracked sequence. One failure skips every later phase; a retry repeats completed phases. | Persist phase completion in `NativeCoreProcess`, attempt every currently safe/incomplete phase, and remove the route only when all phases finish. |
| P0 | `service.rs::ExtensionHost::dispose_session_resources` (currently 3879-3930) | Removes `extension_sessions[key]` before work, then `?` stops on the first process/VM error. The retry handle and unvisited ids are lost. | Retain the record; remove individual ids only after success/confirmed absence; attempt ids in set order; remove the record only when empty. |
| P1 | `service.rs::{dispose_session,dispose_extension_session_state,remove_connection}` (currently 2206-2244 and 2527-2622) | Each loop visits all top-level entries but stores only `first_error`. Requested close removes the session even on failure. | Return a deterministic typed aggregate. Requested close retains non-routable cleanup state; disconnect performs the exhaustive one-shot cleanup and reports the aggregate for logging. |
| P1 | `service.rs::close_session_request` (currently 2359-2509) | Records a failed cleanup as a terminal close outcome after the live session has been removed. Every retry replays the old string instead of retrying cleanup. | Record only successful terminal outcomes. A failed close leaves cleanup state and the next close drives it again. |
| P1 | `vm.rs::{dispose_vm_internal,terminate_vm_processes,finish_vm_teardown}` (currently 937-1117) | Top-level termination hides teardown via `terminate_result?`; the kill loops and teardown helper also stop at their first error. | Attempt all independent teardown phases and return one stable aggregate in phase/resource order. |
| P1 | `stdio.rs::run_async` and `cleanup_connections` (currently 255-377 and 616-625) | Any `?`/writer `return Err` in the event loop skips cleanup. Clean EOF runs cleanup but drops its result with `let _ =`. | Run cleanup on every loop exit and log each connection aggregate with connection id and stable code. Preserve the original transport error as the function result. |
| P2 | `vm.rs::shutdown_configured_mounts` and `dispose_vm_internal` (currently 1412-1472 and 976-990) | Mount failures are converted to an event whose emission result is discarded; cwd/staging `remove_dir_all` results are also discarded. | Aggregate the cleanup errors or emit a host-visible error log with VM/path/plugin identity. No fallible operation may remain as `let _ =`. |

### Retry trap to cover explicitly

`release_native_agent_route` currently stops the output buffer before disposing
extension resources. If a later resource cleanup removes the VM but reports a
post-detach error, the next call repeats `stop_buffering_process_output`; that
method calls `require_owned_vm`, so the retry can now fail forever even though
the output phase already completed. Persisted phase bits are required, not just
moving the `core_processes.remove` line.

## Exact patch checklist

### A. Add the native and shared-core aggregate variants

- [ ] In `crates/native-sidecar/src/state.rs`, add:

  ```rust
  Cleanup {
      context: &'static str,
      errors: Vec<SidecarError>,
  },
  ```

  Match the existing `BrowserSidecarError::Cleanup` display contract: render
  `context`, followed by `; cleanup error N: ...` in vector order. The vector
  must be non-empty.
- [ ] Add a private native helper such as
  `cleanup_result(context, errors) -> Result<(), SidecarError>`. It returns
  `Ok(())` only for an empty vector and returns `Cleanup` for **one or more**
  children. Never change error shape based on cardinality.
- [ ] When a child error does not already name its resource, wrap it before
  adding it so the rendered child names the phase and identity: terminal id,
  process id, VM id, extension namespace, or session id.
- [ ] Map `SidecarError::Cleanup` to `cleanup_failed` in
  `crates/native-sidecar/src/execution.rs::error_code` and
  `crates/agentos-sidecar/src/acp_extension.rs::error_code`.
- [ ] Add `AcpCoreError::Cleanup { context: &'static str, errors:
  Vec<AcpCoreError> }` in `crates/agentos-sidecar-core/src/lib.rs`, with the same
  ordered display and code `cleanup_failed`.
- [ ] Update `acp_extension.rs::sidecar_to_core_error` to recursively preserve a
  native `Cleanup` aggregate as `AcpCoreError::Cleanup`; do not flatten it to
  `Execution(String)`.
- [ ] Let compiler exhaustiveness identify remaining native maps, then verify
  every map preserves `cleanup_failed`; do not bulk-convert cleanup to
  `execution_error`.

### B. Make native ACP route finalization phase-aware

- [ ] Extend `NativeCoreProcess` in `acp_extension.rs` with private cleanup
  progress, initialized false when the process route is inserted:

  ```rust
  struct NativeRouteCleanupProgress {
      output_buffer_stopped: bool,
      terminals_cleaned: bool,
      session_resources_disposed: bool,
  }
  ```

  A process without a bound ACP `session_id` treats the last two phases as
  already complete.
- [ ] Keep the existing `ReleaseAgentRoute` broker command and
  `AcpHost::release_agent_route` hook. Update their comments to call it orderly
  host finalization, because native cleanup now legitimately lives there.
- [ ] Refactor `release_native_agent_route` to clone the route/progress before
  awaits and commit each successful phase back to the exact `(owner_id,
  process_id)` entry. Never hold the `tokio::Mutex` guard across an await.
- [ ] Attempt output-buffer cleanup and every terminal cleanup that is safe in
  the current attempt; collect both errors in phase order. Mark each phase only
  after it returns `Ok`.
- [ ] Treat extension resource disposal as dependent on output/terminal cleanup
  when it may dispose their VM. Do not destroy the VM while a still-retryable
  output/terminal phase needs it. Once prerequisites are complete, drive
  resource disposal and record its error after the earlier phase errors.
- [ ] On a resource-disposal error, retain the process route and its completed
  phase bits. A retry skips completed output/terminal work and re-enters only
  resource disposal.
- [ ] Remove `core_processes[route_key]` only when all applicable bits are true.
  The existing shared core will then remove its ACP session. An aggregate error
  leaves both records present but the close remains fail-closed because the core
  already removes pending prompts at close start.
- [ ] Convert `cleanup_session_terminals` from `Execution(errors.join("; "))`
  to `SidecarError::Cleanup`. Keep terminal-id order and kill-before-output order.
- [ ] In `NativeCoreCommand::AbortAgent`, aggregate kill and route-finalization
  errors rather than formatting them into `AcpCoreError::Execution`. Retain the
  route whenever finalization is incomplete.

### C. Preserve `ExtensionSessionResources` until cleanup completes

- [ ] In `service.rs::dispose_session_resources`, read and clone the sorted
  `process_ids`/`vm_ids`; do not call `extension_sessions.remove(&key)` before
  I/O.
- [ ] Attempt every process id in `BTreeSet` order. Process absence is success.
  Remove an id from the live record after absence or a successful termination
  request; retain it after an error. Record errors with the process id.
- [ ] Do not begin destructive VM cleanup while failed process ids remain for
  that resource record. On a later retry, process cleanup runs first again.
- [ ] Attempt every eligible VM id in `BTreeSet` order. VM absence is success.
  After `dispose_vm_internal` returns, remove the id when the VM is now absent,
  even if teardown returned an error: post-detach teardown cannot be retried and
  retaining that id manufactures an endless ownership error. Retain the id only
  if the VM is still present.
- [ ] Be robust to `dispose_vm_internal` calling `prune_extension_vm_resource`
  and deleting/mutating the same record. Re-fetch `extension_sessions.get_mut`
  after each await; absence means the sidecar already reclaimed the handle.
- [ ] Remove the whole record only when both sets are empty. Return the current
  attempt's ordered aggregate even if every handle became irreversibly absent;
  the next call then sees no record, succeeds, and lets ACP finalization finish.
- [ ] Keep event ordering deterministic. Do not lose events from successful VM
  cleanups merely because a sibling failed; if the current `Result<Vec<_>, _>`
  signature cannot carry both, introduce an internal outcome containing
  `events` plus `errors` and convert only at the wire boundary.

### D. Aggregate native service and VM disposal

- [ ] Replace `first_error` in `remove_connection`, `dispose_session`, and
  `dispose_extension_session_state` with ordered vectors and the common helper.
  Preserve ordering: connection session ids; session VM ids; then extension
  namespaces.
- [ ] Collect `(namespace, Arc<dyn Extension>)` rather than values alone in
  `dispose_extension_session_state`, so each child names the failing namespace.
- [ ] For requested wire close, add a non-routable cleanup lifecycle/progress to
  `SessionState` (in `state.rs`). `require_owned_session` and VM creation/runtime
  requests must reject a disposing session; `close_session_request` alone may
  re-enter its cleanup driver.
- [ ] Retain which extension namespaces completed successfully, so a retry does
  not call successful hooks twice. Existing `session.vm_ids` is the pending VM
  set because `reclaim_vm_tracking` removes a detached VM id.
- [ ] Remove the session, connection membership, and push `disposed_sessions`
  only after a requested cleanup attempt completes without errors. If an
  irreversible post-detach error emptied all pending phases, return the failure
  once; the next close finalizes the empty cleanup state.
- [ ] In `close_session_request`, record `SessionCloseOutcome` only after final
  success. Never store `error_message: Some(...)`; a failure is retryable, not a
  terminal outcome. Second close retries; third close replays the successful
  bounded history entry.
- [ ] For `DisposeReason::ConnectionClosed`, attempt every incomplete phase once,
  aggregate failures, then remove routable connection/session state. There is no
  caller retry. If a concrete resource handle must survive for retry, move it to
  a bounded non-routable cleanup registry; do not leave an active session owned
  by a removed connection.
- [ ] In `vm.rs::terminate_vm_processes`, collect failures for every SIGTERM
  target, the first wait, every remaining SIGKILL target, the second wait, and
  the final active-process check. Continue whenever the next operation is
  independent and keep process-id/phase order.
- [ ] In `finish_vm_teardown`, run independent phases even after an earlier
  failure: snapshot/encode, lifecycle emission, kernel dispose, snapshot flush
  when a snapshot exists, and permission clearing. Aggregate in that order.
- [ ] In `dispose_vm_internal`, aggregate mount shutdown, process termination,
  teardown, cwd removal, and staging-root removal after unconditional
  `reclaim_vm_tracking`. Treat `NotFound` directory removal as success; include
  other I/O errors with the exact path.
- [ ] Change `shutdown_configured_mounts` so continue-on-error mode collects
  unmount failures instead of returning `Ok(())`. Always emit a `tracing::error!`
  containing VM id, guest path, plugin id, phase, errno, and error. If the
  structured failure event also cannot be emitted, log that second failure;
  remove the current discarded `let _ = emit_structured_event(...)`.

### E. Make stdio cleanup unconditional and visible

- [ ] In `stdio.rs::run_async`, execute the event loop inside an async block that
  produces `run_result`. After the block finishes for EOF **or any error**, call
  `cleanup_connections`, then return the original `run_result`. Cleanup must not
  replace a more useful transport/dispatch error.
- [ ] In `cleanup_connections`, retain `BTreeSet` connection order and match each
  `remove_connection` result. For every failure, emit:

  ```rust
  tracing::error!(
      target: "agentos_native_sidecar::stdio",
      connection_id,
      error_code = crate::execution::error_code(&error),
      error = %error,
      "failed to clean up disconnected native-sidecar connection",
  );
  ```

- [ ] Continue to every connection after a failure, then untrack every disposed
  session. Remove `let _ = sidecar.remove_connection(...)`.
- [ ] Do not attempt to send cleanup failure frames to stdout after disconnect;
  stderr/tracing is the authoritative host-visible path.

## Focused before/after tests

Add the named tests before changing behavior and record the old outcomes in the
tracker. All assertions must inspect the typed variants and vector order, not
only substring presence.

### `crates/native-sidecar/src/state.rs` / `execution.rs`

- [ ] `cleanup_error_code_and_display_are_stable_and_ordered`: construct one and
  two-child native aggregates; both have `cleanup_failed`, both retain the
  `Cleanup` shape, and display preserves insertion order.

### `crates/agentos-sidecar/src/acp_extension.rs`

- [ ] Extract a small fakeable cleanup driver around native route phases and add
  `native_route_finalization_retries_only_incomplete_phases`. First attempt:
  output succeeds, terminal succeeds, resources fail once. Assert the route and
  core session remain. Second attempt calls only resources and succeeds; route
  and core session disappear. Third close is idempotent and calls no phase.
- [ ] `native_route_finalization_preserves_phase_error_order`: fail output and
  two terminal actions in one safe attempt. Assert one `AcpCoreError::Cleanup`
  with output first, terminal ids in `BTreeMap` order, and kill before output for
  one terminal. Assert destructive VM/resource cleanup was deferred while its
  prerequisites remained incomplete.
- [ ] `native_abort_preserves_kill_and_finalization_failures`: fail both branches
  and assert one ordered aggregate, with the route retained for retry.

### `crates/native-sidecar/src/service.rs`

- [ ] Extend the existing `dispose_lifecycle_tests::RecordingExtension` into a
  programmable extension with namespace, call count, and failures remaining.
- [ ] `dispose_extension_session_state_aggregates_namespace_order_and_retries_only_failures`:
  register two failing extensions in reverse insertion order, verify namespace
  order in the aggregate, clear failures, and verify only unfinished namespaces
  are called again.
- [ ] `extension_resource_cleanup_retains_failed_ids_and_retries_only_incomplete_ids`:
  use a local fake cleanup seam for two process ids and two VM ids. Fail the
  second of each once. Verify every eligible sibling is attempted, successes
  are removed, failures remain, and retry calls only retained ids.
- [ ] `extension_resource_cleanup_drops_post_detach_vm_handle_but_reports_current_error`:
  make VM disposal detach then fail. The first result contains the VM error and
  the id is absent; retry succeeds without an ownership error.
- [ ] Replace the current
  `dispose_session_reclaims_session_even_when_a_vm_dispose_fails` expectation.
  Requested cleanup failure must retain a **non-routable** cleanup session;
  successful retry removes it. ConnectionClosed still detaches it after the
  exhaustive attempt.
- [ ] `close_session_request_retries_cleanup_failure_before_recording_terminal_success`:
  a one-shot extension failure rejects the first close with `cleanup_failed`,
  leaves no terminal history entry, succeeds on the second close, and replays
  success without new cleanup on the third.
- [ ] `remove_connection_aggregates_every_session_failure_before_detach`: seed
  two sessions with ordered failing extensions/VM identities. Assert all
  sentinels occur once in session/resource order and connection/session routing
  state is gone afterward.

### `crates/native-sidecar/src/vm.rs`

- [ ] Add a small teardown-operations fake rather than environment variables.
  `dispose_vm_aggregates_process_teardown_mount_and_filesystem_failures` injects
  at least one failure in each independent phase and asserts exact vector order,
  one occurrence each, VM detachment, and zero per-VM tracking entries.
- [ ] `mount_shutdown_logs_primary_and_event_delivery_failure` captures tracing
  and verifies VM/plugin/guest-path/errno identity even when structured-event
  delivery also fails.

### `crates/native-sidecar/src/stdio.rs`

- [ ] Add a tiny `tracing_subscriber::fmt` test writer backed by
  `Arc<Mutex<Vec<u8>>>` (the dependency already exists).
- [ ] `cleanup_connections_logs_connection_id_code_and_every_child`: seed an
  authenticated connection whose two session cleanup hooks fail. Invoke
  `cleanup_connections`; assert the captured stderr contains the connection id,
  `cleanup_failed`, both sentinels in deterministic order, and no live
  connection/session route.
- [ ] `event_loop_error_still_runs_disconnect_cleanup`: factor the event-loop
  body so a synthetic writer/read error can terminate it, then assert the same
  cleanup hook ran before `run_async` returned the original error.

## Validation and revision boundary

Expected native implementation paths:

- `crates/agentos-sidecar-core/src/lib.rs`
- `crates/agentos-sidecar-core/src/host.rs` (comment only unless the existing
  hook contract genuinely cannot carry finalization)
- `crates/agentos-sidecar/src/acp_extension.rs`
- `crates/native-sidecar/src/state.rs`
- `crates/native-sidecar/src/execution.rs`
- `crates/native-sidecar/src/service.rs`
- `crates/native-sidecar/src/vm.rs`
- `crates/native-sidecar/src/stdio.rs`
- their focused native tests

Do not touch client packages. Keep Item 36 in its dedicated child `jj` revision.

Run:

```sh
cargo test -p agentos-sidecar-core --lib
cargo test -p agentos-sidecar --lib acp_extension::tests -- --nocapture
cargo test -p agentos-native-sidecar --lib dispose_lifecycle_tests -- --nocapture
cargo test -p agentos-native-sidecar --lib stdio::tests -- --nocapture
cargo test -p agentos-native-sidecar --lib vm:: -- --nocapture
cargo check --workspace
cargo fmt --all -- --check
git diff --check
```

Item 36 native completion requires all of the following:

- [ ] every independent native cleanup child is represented once in a stable
  typed aggregate;
- [ ] an ACP close failure retains only non-routable handles and retries only
  incomplete phases;
- [ ] extension process/VM resource ids survive until success or confirmed
  absence;
- [ ] failed requested wire close is not entered into terminal history;
- [ ] all stdio exit paths run disconnect cleanup and any cleanup failure is
  visible on stderr with its connection id;
- [ ] no cleanup-related `let _ =`, `first_error`, `errors.join`, or early `?`
  remains in the audited native paths unless the skipped operation is explicitly
  dependent and documented.
