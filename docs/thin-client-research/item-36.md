# Item 36 research: ACP discovery and cleanup failures

Status: research only. This note does not change `docs/thin-client-migration.md`
or any production/test implementation.

## Outcome

Item 36 should be implemented as a sidecar-only error-propagation and cleanup
transaction change. No TypeScript or Rust client behavior is needed.

The discovery half is nearly implemented already by the in-progress Item 34
ACP convergence: the shared core now preserves errors returned by the host's
projected-agent source. Item 36 still needs focused regressions that distinguish
an authoritative projected-state failure from a valid empty catalog or a real
unknown-agent result.

The cleanup half is not complete. There are four related defects:

1. Native and browser session/extension cleanup loops attempt multiple actions
   but retain only `first_error`, hiding every later failure.
2. Native ACP close removes the shared-core session before native terminal and
   extension-resource cleanup runs. If that cleanup fails, retry is reported as
   an idempotent success and the cleanup is not retried.
3. Browser execution cleanup removes the only worker handle before fallible
   kernel/worker cleanup. A failed termination is reported once but cannot be
   retried; the ACP route has also already been removed.
4. Native stdio connection teardown discards the result of
   `remove_connection`, so all aggregated disconnect-cleanup failures can still
   disappear without a caller response or a log.

The implementation should make cleanup state non-routable immediately, retain
only the opaque cleanup handle/phases needed for retry, try every independent
cleanup action in deterministic order, and return one typed aggregate. When no
request remains to receive the aggregate (stdio disconnect/process shutdown),
log it at the failure site.

## Current code map

Line numbers below are from the current Item 34 working copy and may move. The
function names are the authoritative anchors.

### Discovery

| Layer | File / symbol | Current behavior |
| --- | --- | --- |
| Shared core | `crates/agentos-sidecar-core/src/engine.rs::resolve_agent` (around lines 69-89) | Correct in the current working copy: `host.resolve_projected_agent(agent_type)?` propagates a host failure unchanged; only `Ok(None)` or an empty entrypoint becomes the stable unknown-agent error. |
| Shared core | `AcpCore::list_agents` in the same file | Calls `host.list_projected_agents()?`; a host failure is not a valid empty list. |
| Native host adapter | `crates/agentos-sidecar/src/acp_extension.rs::handle_native_core_command`, `NativeCoreCommand::{ResolveAgent,ListAgents}` (around lines 531-580) | Correct in the current working copy: `ctx.projected_agents().await.map_err(sidecar_to_core_error)` preserves the failure. |
| Native projected source | `crates/native-sidecar/src/service.rs` `ExtensionHost::projected_agents` (around lines 3789-3817) | Returns ownership/VM lookup errors and the live sidecar-owned projected launch state. |
| Browser host adapter | `crates/agentos-sidecar-browser/src/acp_host.rs::resolve_projected_agent` and `list_projected_agents` (around lines 117-131) | Correct in the current working copy: maps `BrowserSidecarError` to a semantic `AcpCoreError`; no empty fallback. |
| Browser projected source | `crates/native-sidecar-browser/src/service.rs::resolve_projected_agent` and `list_projected_agents` | Reads the VM's sidecar-owned projected launch map and returns `InvalidState` for an absent VM. |

The pre-Item-34 native code at Item 33 revision `066f6b51` demonstrates the
original defect exactly:

- `AcpExtension::list_agents` used
  `ctx.projected_agents().await.unwrap_or_default()`, converting failure into a
  successful empty list.
- `read_projected_agent_block` used
  `ctx.projected_agents().await.ok()?`, converting the same failure into
  `None`; `resolve_agent` then reported the package as an unknown agent.

Do not reintroduce a guest-filesystem manifest read to solve this. The source of
truth remains the sidecar-owned live projection decoded from `.aospkg` metadata.

### Shared-core cleanup

| File / symbol | Defect |
| --- | --- |
| `crates/agentos-sidecar-core/src/engine.rs::AcpCore::close_session` (around lines 503-572) | The core correctly retains the session through `close_stdin`, signal, wait, and `release_agent_route`, but the native-only cleanup still happens after this function returns and after line 565 removes the core session. |
| `AcpCore::dispose_owner` (around lines 1883-1906) | It tries every process in deterministic `BTreeSet` order, but flattens failures into one `Execution(String)` and calls `take_owner_state` before cleanup. A retry has no list of processes to clean. |
| `abort_agent_for_cleanup` (around lines 3034-3043) | Every bootstrap/collision/event/bind/restart cleanup failure is warning-only. Call sites return only the primary error even though the adapter cleanup may also have failed. |
| `AcpCore::abort_pending` | Removes pending state before `host.abort_agent`; the existing test `resumable_abort_removes_pending_state_before_cleanup_error_propagates` proves that retry becomes `invalid_state`, not a retry of cleanup. |

`abort_agent_for_cleanup` is used by blocking create/resume, resumable
create/resume/prompt/restart, and restart fallback paths. Replace the helper; do
not fix only close-session and leave these cleanup failures warning-only.

### Native ACP wrapper and native host cleanup

| File / symbol | Defect |
| --- | --- |
| `crates/agentos-sidecar/src/acp_extension.rs::dispatch_shared_core` (around lines 441-453) | After the core has removed the session, terminal cleanup returns early on its first aggregate, then extension-resource cleanup is skipped. If resource cleanup fails, the core session is already gone. |
| `AcpExtension::cleanup_session_terminals` (around lines 828-876) | It retains failed terminal records and tries kill plus output cleanup, which is good, but returns an untyped `SidecarError::Execution` string. The caller then skips the next independent cleanup phase. |
| `crates/native-sidecar/src/service.rs` `ExtensionHost::dispose_session_resources` (around lines 3876-3927) | Removes `extension_sessions[key]` before fallible work and uses `?` in both loops. A first process/VM error hides later failures and destroys the retry handle. |
| `NativeSidecar::dispose_extension_session_state` (around lines 2591-2619) | Invokes every extension but returns only the first error. Iteration is deterministic because `extensions` is a `BTreeMap`; the later errors should be retained in that order. |
| `NativeSidecar::dispose_session` (around lines 2530-2585) | Invokes all VMs and the extension hook but saves only the first error, removes the session on failure, and makes the failed close terminal. |
| `NativeSidecar::remove_connection` (around lines 2204-2241) | Invokes every session but saves only the first error and removes the connection. |
| `crates/native-sidecar/src/stdio.rs::cleanup_connections` (around lines 612-620) | `let _ = sidecar.remove_connection(...).await` silently drops the final cleanup error. This path has no request to reject, so it must use `tracing::error!` with the connection id and aggregate. |
| `crates/native-sidecar/src/vm.rs::dispose_vm_internal` | Captures termination and teardown, then `terminate_result?; teardown_result?` reports only the first. It also intentionally discards configured-mount shutdown because that helper logs internally. For Item 36, aggregate termination and teardown; keep the logged mount policy only if every individual mount failure is demonstrably logged with identity. |

The native wire close path in
`NativeSidecar::close_session_request` stores an error in terminal close history
after `dispose_session` has removed the live session. Retrying therefore replays
the old failure instead of retrying remaining cleanup. Successful terminal
history is useful; a failed attempt is not a terminal outcome until cleanup is
complete.

### Browser ACP wrapper and browser host cleanup

| File / symbol | Defect |
| --- | --- |
| `crates/agentos-sidecar-browser/src/acp_host.rs::BrowserAcpHost::abort_agent` (around lines 247-255) | Removes the ACP process route before `abort_execution`. Fail-closed routing is correct, but no separate cleanup handle remains if host cleanup fails. |
| `crates/agentos-sidecar-browser/src/lib.rs::BrowserAcpExtension::dispose_owners` (around lines 82-108) | Tries owners but flattens failures to `InvalidState(String)`; `AcpCore::dispose_owner` has already discarded owner state. |
| `crates/native-sidecar-browser/src/service.rs::dispose_extension_session_state` and `dispose_extension_vm_state` (around lines 688-752) | Both invoke every extension but use `first_error`, hiding later extension failures. |
| `BrowserSidecar::abort_execution` (around lines 2219-2239) | Attempts kill and release, but the two-error case is flattened to `InvalidState` instead of the existing typed `BrowserSidecarError::Cleanup`. |
| `BrowserSidecar::release_execution` (around lines 2475-2544) | Removes `ExecutionState` before reaping the kernel process and terminating the worker. It aggregates the two immediate failures, but retry sees no execution and returns success without reattempting termination. |
| `BrowserSidecar::dispose_vm` (around lines 1829-1880) | Correctly attempts all active executions, but it depends on early removal in `release_execution`; a failure drains the active map and loses the handle. It also returns a raw error for one failure and `Cleanup` for two, so cleanup error shape changes with cardinality. |
| `crates/native-sidecar-browser/src/wire_dispatch.rs::close_session` (around lines 1382-1446) | Keeps only the first extension/VM/session error, purges ownership state, removes the session, and records the failure as terminal. Retry only replays that first string. |
| `BrowserWireDispatcher::dispose_vm` (around lines 1720-1769) | Correctly observes both extension and VM failures, but formats them ad hoc; it should use the same typed aggregate helper. |

`BrowserSidecarError::Cleanup { context, errors }` already exists and its
`Display` implementation preserves vector order. Reuse it rather than creating
another browser cleanup string format.

## Recommended implementation

### 1. Add one typed aggregate shape per error layer

Add `Cleanup { context: &'static str, errors: Vec<...> }` to native
`SidecarError` and shared `AcpCoreError`, matching the existing browser error.
The vector must be non-empty and kept in operation order. Add a small helper at
each layer that returns `Ok(())` for an empty vector and always returns the
`Cleanup` variant for one or more errors; do not return a raw child for the
single-error case.

Recommended stable codes:

- `AcpCoreError::Cleanup` -> ACP code `cleanup_failed`.
- native `SidecarError::Cleanup` -> native error code `cleanup_failed`.
- `BrowserSidecarError::Cleanup` already maps to browser wire code
  `cleanup_failed`.
- wire-session close may retain its operation code `close_session_failed`, with
  the nested aggregate in the message/details.

Update these exhaustive maps:

- `crates/agentos-sidecar-core/src/lib.rs::{AcpCoreError::code, Display}`.
- `crates/agentos-sidecar/src/acp_extension.rs::{sidecar_to_core_error,error_code}`.
- `crates/native-sidecar/src/execution.rs::error_code` and any exhaustive native
  error matches found by `rg 'match .*error|SidecarError::'`.
- `crates/agentos-sidecar-browser/src/acp_host.rs::map_err` should map browser
  `Cleanup` to shared-core `Cleanup`, not generic `Execution`.
- `BrowserAcpExtension::dispose_owners` should return
  `BrowserSidecarError::Cleanup`, retaining the child errors rather than their
  strings.

Keep operation identity in each child message, for example:

```text
failed to clean up native ACP session "s1";
cleanup error 1: terminal "term-a" kill: ...;
cleanup error 2: terminal "term-b" output cleanup: ...;
cleanup error 3: extension session resources: ...
```

Use stable ordering:

1. adapter close/signal/wait;
2. terminal ids in `BTreeMap` order, kill before output cleanup;
3. extension process ids in `BTreeSet` order;
4. extension VM ids in `BTreeSet` order;
5. extension namespaces in `BTreeMap` order;
6. session VM ids and session ids in their existing `BTreeSet` order.

Never sort rendered error strings; sort/iterate resource identities and preserve
the semantic phase ordering above.

### 2. Move native post-close cleanup into the shared core host transaction

Replace the narrow host hook
`AcpHost::release_agent_route(process_id)` with an idempotent finalization hook
that receives both identities, for example:

```rust
fn finalize_session_cleanup(
    &mut self,
    session_id: &str,
    process_id: &str,
) -> Result<(), AcpCoreError>;
```

`AcpCore::close_session` should call this hook on every owned close, including an
adapter already marked closed, and only then remove the session. A hook failure
therefore leaves the authoritative core record in place for retry. The browser
implementation removes the now-inactive ACP route idempotently. The native
implementation sends a new broker command carrying `session_id` and
`process_id`; its async handler:

1. runs `cleanup_session_terminals`;
2. always runs `dispose_session_resources_wire`, even if step 1 failed;
3. aggregates both results;
4. removes matching `core_processes` metadata only after all cleanup succeeds.

Delete the post-core close block in `dispatch_shared_core` once this is in the
host hook. Leaving both creates a double-cleanup path and preserves the retry
bug.

### 3. Preserve native resource retry state

Refactor `ExtensionHost::dispose_session_resources` so it does not remove the
entire `ExtensionSessionResources` record before work:

1. Clone the sorted process/VM ids to drive cleanup without holding a mutable
   map borrow across async calls.
2. Attempt every process and VM.
3. Remove an individual id from the retained record when that resource is
   confirmed absent or cleanup succeeds.
4. If a VM teardown returned an error but the VM was nevertheless detached,
   remove that VM id (there is nothing left to retry) and retain the error in
   the aggregate.
5. Remove the record only when both retained sets are empty.

This makes a native ACP close retry meaningful. It also prevents one failed
terminal/process cleanup from hiding later failures.

For wire-session close, do not put a failed attempt in terminal close history
or remove the session until retryable cleanup is complete. If a particular
resource is irreversibly gone despite reporting an error, mark that phase done,
retain/log the error for the current attempt, and let the next close finish the
remaining phases. Only a successful terminal result belongs in the bounded
close history.

Connection loss has no caller retry. Attempt all retained cleanup once more,
log the typed aggregate with exact connection/session ownership, then detach
routable state. If handles must outlive the connection for background retry,
store them in a bounded, non-routable sidecar cleanup registry and drain that
registry during sidecar shutdown.

### 4. Preserve browser cleanup handles without preserving routes

Do not keep a failed execution in the active/routable ACP map. Add a separate
sidecar-owned cleanup record (or an explicit non-routable lifecycle state) that
retains:

- VM id, execution id, worker id, runtime, and kernel pid;
- whether kernel reap completed;
- whether worker termination completed;
- whether structured and lifecycle cleanup events completed;
- the stable cleanup event name.

`release_execution` should atomically move an active execution into this cleanup
state before fallible work, attempt every incomplete phase, mark successful
phases, and remove the cleanup record only when all phases finish. A retry calls
the same phase driver. `ensure_execution`, stdin, signal, and polling APIs must
reject a cleanup-state id as non-active so retaining the handle cannot make the
execution routable again.

The cleanup registry must be bounded and must warn near capacity. The cleanest
bound is a `BrowserSidecarConfig` per-VM pending-cleanup limit enforced at
execution admission; the `LimitExceeded` error should name
`max_pending_execution_cleanups_per_vm` and how to raise it. Count active plus
cleanup-state executions for admission so repeated permanent termination
failures cannot grow the registry by starting replacements.

`abort_execution`, `dispose_vm`, and session close should drive pending cleanup
records too. If VM state has already been detached, kernel cleanup is complete
by destruction but the worker handle still must be retried. A VM cleanup
tombstone is also needed when the only failed phase is terminal event emission;
otherwise an empty VM has no execution record on which to retain that failure.

Browser wire session close must keep the live session/VM cleanup ownership until
all extension and VM cleanup phases finish. Do not record a failed attempt as a
terminal close outcome. Record and replay only the final successful close (or a
true terminal outcome that has no remaining cleanup handle).

### 5. Replace warning-only shared-core abort cleanup

Change `abort_agent_for_cleanup` to return an error aggregate instead of `()`.
At every call site, combine the primary failure and abort failure in fixed order.
For resumable paths, retain a bounded owner/process cleanup tombstone when host
abort fails. A subsequent `AcpAbortPendingRequest` for that exact owner/process
must retry host cleanup and never expose the tombstone to another owner.

The tombstone is not an active interaction: it must not accept output or session
requests. It exists only to retain the host cleanup correlation and the terminal
response that should be returned when retry succeeds. Include it in diagnostics
as `pending_cleanup_count` so wrapper tests can prove zero residual resources.

Use a sidecar-resolved bound with a near-capacity warning and typed
`limit_exceeded` admission failure. Do not introduce an unbounded cleanup map.

## Focused tests

### Before-behavior evidence

Add named regression tests before changing the implementation, and run them
against Item 33 (`066f6b51`) or encode the old helper behavior in a focused
fixture. The tracking row should name the tests and record the observed old
outcome.

1. In `crates/agentos-sidecar/tests/acp_extension.rs`, add
   `acp_list_agents_preserves_projected_state_failure`. Dispatch a valid
   `AcpListAgentsRequest` with session ownership but no VM scope. The projected
   source returns the exact ownership error. At Item 33 the request succeeds
   with `agents: []`; after the fix it must be `AcpErrorResponse` with
   `invalid_state` and the original "requires VM ownership" message.
2. In `crates/agentos-sidecar-core/src/engine.rs`, use a host whose
   `resolve_projected_agent` and `list_projected_agents` return distinct sentinel
   errors. Assert create/resume and list return those exact code/message pairs,
   never unknown-agent/empty.
3. Extend the existing native service disposal test fixtures with two failing
   extensions and two failing VM/resource identities. The pre-fix result contains
   only the first sentinel; after the fix it contains every sentinel once in the
   documented order.
4. Replace/rename the current core test
   `resumable_abort_removes_pending_state_before_cleanup_error_propagates`.
   Before: cleanup failure removes pending state and retry is `invalid_state`.
   After: pending interaction is non-routable, a cleanup tombstone remains,
   exact-owner retry reattempts abort, and diagnostics return to zero.
5. Add a native ACP close test where final session cleanup fails once and then
   succeeds. The first close must return `cleanup_failed`; state/cleanup
   correlation remains. The second close must reattempt the failing phase and
   return closed; a third close is idempotent and performs no cleanup.
6. In `crates/native-sidecar-browser/src/service.rs`, update the existing tests
   `release_execution_terminates_worker_after_kernel_cleanup_failure` and
   `release_execution_preserves_both_cleanup_errors_after_draining_maps`.
   Assert the active route count is zero but a non-routable cleanup record exists,
   both sentinels are present in order, retry calls only incomplete phases, and
   cleanup count reaches zero.
7. In `crates/native-sidecar-browser/src/wire_dispatch.rs`, register two failing
   extensions and a bridge/kernel cleanup failure. Assert close returns one
   deterministic aggregate containing all failures, retains retry ownership,
   and a subsequent close completes after failures are cleared.
8. In `crates/agentos-sidecar/tests/acp_wrapper_conformance.rs`, add native and
   browser wrapper parity for one-shot cleanup failure -> retry success -> zero
   sessions/pending interactions/process routes/cleanup tombstones.
9. Add a native stdio unit test around `cleanup_connections` with a failing
   sidecar cleanup seam or extracted reporter. Assert a host-visible tracing/log
   record contains connection id plus every cleanup child. Do not test only that
   the function returns; stdio intentionally has no receiver for that result.

Avoid a test-only production default or environment variable. Prefer small
failing host/extension/bridge implementations and existing `#[cfg(test)]`
failure seams.

### Suggested commands

```sh
cargo test -p agentos-sidecar-core --lib
cargo test -p agentos-sidecar --test acp_extension -- --nocapture
cargo test -p agentos-sidecar --test acp_wrapper_conformance -- --nocapture
cargo test -p agentos-native-sidecar --lib service::tests -- --nocapture
cargo test -p agentos-native-sidecar --lib stdio:: -- --nocapture
cargo test -p agentos-native-sidecar-browser --lib -- --nocapture
cargo test -p agentos-sidecar-browser --lib -- --nocapture
cargo check -p agentos-sidecar-browser --target wasm32-unknown-unknown
cargo check --workspace
cargo fmt --all -- --check
git diff --check
```

Run the wrapper conformance suite more than once because it exercises real V8
process teardown and has historically exposed late-event races.

## Dedicated Item 36 revision scope

Create Item 36 as one new child `jj` revision after Item 35 is sealed. Do not
fold it into Item 34 or Item 35. Expected paths are:

- `crates/agentos-sidecar-core/src/lib.rs`
- `crates/agentos-sidecar-core/src/host.rs`
- `crates/agentos-sidecar-core/src/engine.rs`
- `crates/agentos-sidecar/src/acp_extension.rs`
- `crates/agentos-sidecar/tests/acp_extension.rs`
- `crates/agentos-sidecar/tests/acp_wrapper_conformance.rs`
- `crates/native-sidecar/src/state.rs`
- `crates/native-sidecar/src/service.rs`
- `crates/native-sidecar/src/vm.rs` if VM teardown aggregation is included
- `crates/native-sidecar/src/stdio.rs`
- `crates/native-sidecar-browser/src/service.rs`
- `crates/native-sidecar-browser/src/wire_dispatch.rs`
- `crates/agentos-sidecar-browser/src/acp_host.rs`
- `crates/agentos-sidecar-browser/src/lib.rs`
- `docs/thin-client-migration.md` only when tests and the dedicated revision are
  complete

No client package path should change. If implementation starts touching
`packages/core`, `packages/browser` protocol-driving logic, or the Rust SDK,
stop and re-check the boundary: the clients should only receive the same typed
sidecar response and retain their already-required remote-disposal retry route.

## Risks and implementation checks

- **Do not retain routable failed state.** Cleanup tombstones need exact
  ownership and resource handles, not session/prompt APIs.
- **Keep every new collection bounded.** Permanent bridge termination failure is
  attacker-influenced via hostile guest execution and cannot create an unbounded
  tombstone registry.
- **Do not retry completed phases.** A second `terminate_worker` or a second
  lifecycle event can create false failures/duplicates. Store per-phase progress.
- **Preserve original discovery errors.** Only `Ok(None)` is unknown-agent; an
  `Err` is never absence.
- **Preserve semantic error codes through wrappers.** Do not convert Cleanup to
  `InvalidState` in browser or to a message-only `Execution` in native.
- **Keep cleanup idempotent.** Process-gone/VM-gone after a confirmed cleanup
  phase is success; ownership mismatch is not.
- **Events and cleanup are separate transactions.** Item 34's ACP event
  acknowledgement/retry work should remain intact. Cleanup-event progress must
  not acknowledge an event that was never emitted.
- **Do not move this into clients.** Clients cannot see kernel, worker, mount, or
  extension resource handles and are not the enforcement point.

## Completion evidence required in the tracker

Item 36 is complete only when the tracking row names:

- the before test(s) demonstrating empty/unknown discovery masking and first-only
  or unretryable cleanup;
- the after test(s) proving exact discovery propagation, all-child deterministic
  aggregation, exact-owner retry, zero residual cleanup state, and logged
  disconnect failures;
- the dedicated Item 36 `jj` change id/revision; and
- an independent sub-agent seal review with no unresolved P0/P1/P2 finding.
