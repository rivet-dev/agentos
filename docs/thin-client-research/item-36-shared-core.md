# Item 36 supplemental audit: shared-core cleanup retry state

Status: implementation checklist only. This note does not modify production
code, tests, or the Item 36 tracker.

## Exact defects after Item 35

1. `AcpCore::abort_pending` at
   `crates/agentos-sidecar-core/src/engine.rs:2165-2319` removes the active
   pending entry before `host.abort_agent`. If host cleanup fails, a retry sees
   `no pending ACP interaction` and the cleanup handle is lost. The existing
   regression `resumable_abort_removes_pending_state_before_cleanup_error_propagates`
   at `engine.rs:5805-5847` proves this behavior.
2. `abort_agent_for_cleanup` at `engine.rs:3670-3678` only warns. Its create,
   resume, prompt, and restart callers return the primary error while silently
   abandoning a failed host cleanup.
3. Orderly close calls the narrow `AcpHost::release_agent_route` hook. Native's
   implementation in `crates/agentos-sidecar/src/acp_extension.rs:1223-1239`
   runs output, terminal, and extension-session cleanup sequentially with `?`.
   A first failure skips later phases.
4. Native `dispose_session_resources` at
   `crates/native-sidecar/src/service.rs:3879-3930` removes the complete
   `ExtensionSessionResources` record before killing its process/VM resources.
   Any failure leaves nothing for a close retry.
5. Shared `AcpCoreError` has no aggregate cleanup variant, so wrappers flatten
   multiple failures into `Execution(String)` or `InvalidState(String)`.

## Required shared-core data model

Add a bounded, non-routable cleanup registry to `AcpCore`:

```rust
type CleanupKey = (String, String); // exact (owner_id, process_id)

struct PendingCleanup {
    session_id: Option<String>,
    action: CleanupAction,
    completion: CleanupCompletion,
}

enum CleanupAction {
    AbortAgent,
    FinalizeSession { session_id: String },
}

enum CleanupCompletion {
    Response(AcpResponse),
    Error(AcpCoreError),
    RestartPrompt {
        pending: PendingPrompt,
        exit_code: Option<i32>,
    },
}
```

Use `BTreeMap<CleanupKey, PendingCleanup>` plus a bounded secondary
`BTreeMap<(owner_id, session_id), process_id>` only for close retry lookup.
Both maps contain the same bounded records; the second map is an index, not
another lifecycle state machine.

The tombstone must not appear in `pending_response`, accept agent output, accept
session requests, or be returned by `list_sessions`. A wrong owner receives the
same `no pending ACP interaction` response as an absent process. Store the
original completion; a retry may not change it by supplying a different abort
reason.

### Bound and admission invariant

Add a sidecar-owned per-owner route/cleanup limit, defaulted in the shared core
and overridable by the embedding sidecar, with APIs analogous to the event
limit:

```rust
AcpCore::with_process_cleanup_limit(limit)
set_process_cleanup_limit(owner_id, limit)
```

Count unique process IDs across live sessions, all five pending maps, and cleanup
tombstones. Enforce the limit before every operation that can add a new process
ID (`begin_create_session`, `begin_resume_session`, and adapter restart spawn).
A prompt or close reuses its session process ID and consumes no extra slot.

This reservation is critical: a failed process route replaces one already
counted active/pending route with one tombstone, so recording retry state can
never itself fail at cleanup time. Do not check capacity only after
`abort_agent` fails.

Use `AcpCoreError::LimitExceeded` with the exact observed count, limit, and
instruction to raise the sidecar ACP process-cleanup limit. Warn once per owner
near the threshold. Refuse zero or lowering below current usage. Remove owner
limit/warning state only when owner state is fully dropped.

## Exact transition helpers

Implement three helpers in `engine.rs`:

```rust
fn stage_cleanup(&mut self, key: CleanupKey, cleanup: PendingCleanup);

fn drive_process_cleanup<H: AcpHost>(
    &mut self,
    host: &mut H,
    owner_id: &str,
    process_id: &str,
) -> Result<AcpResponse, AcpCoreError>;

fn drive_session_cleanup<H: AcpHost>(
    &mut self,
    host: &mut H,
    owner_id: &str,
    session_id: &str,
) -> Result<AcpResponse, AcpCoreError>;
```

`drive_*` must:

1. verify exact ownership before invoking the host;
2. call only the tombstone's stored action;
3. retain the tombstone unchanged on error;
4. remove both indices only after host success; and
5. then deliver the stored response/error or run the stored restart continuation.

Remove the tombstone before running a restart continuation so a new failure is
recorded by the normal restart path, not by mutating the old tombstone. If the
continuation returns an error, preserve both the completed cleanup result and
that new error normally.

## `abort_pending` patch checklist

- At the very start of `abort_pending`, check `(owner_id, process_id)` in the
  cleanup registry and call `drive_process_cleanup`. This is the retry path.
- Replace `remove_pending_state` with `take_pending_state`, returning an enum
  containing the moved pending value. Apply session mutations (closed flag,
  preamble restore, or session removal) explicitly for that variant.
- Construct the final `AcpResponse` before host cleanup and store it in
  `CleanupCompletion::Response`.
- Insert the tombstone **before** calling `host.abort_agent`; then call
  `drive_process_cleanup` immediately for the first attempt.
- For `AgentExited` during a prompt, store the moved `PendingPrompt` and exit
  code in `RestartPrompt`. Only begin adapter restart after abort succeeds.
- For a failed replacement restart, retain the moved `PendingRestart` until
  abort succeeds; do not render cleanup failure into the restart message and
  discard the handle.
- For pending-close `DriverFailed`/`CallerCancelled`, mark the session closed,
  stage `AbortAgent`, and remove the session only after abort succeeds.
- A tombstoned process is absent from every pending map immediately, so
  `feed_agent_output` and `pending_response` reject it.

Replace `abort_agent_for_cleanup` with a result-bearing staging helper. Every
current call site must return `AcpCoreError::Cleanup` containing the primary
error first and the abort failure second, while leaving a retry tombstone. No
empty `catch`, warning-only result, or string concatenation remains.

## Orderly session finalization checklist

Replace the host seam in `crates/agentos-sidecar-core/src/host.rs:105-110`:

```rust
fn finalize_session_cleanup(
    &mut self,
    session_id: &str,
    process_id: &str,
) -> Result<(), AcpCoreError>;
```

The hook is idempotent and means "finish every host-owned resource for this
closed ACP session," not merely "remove one process route."

In both blocking `close_session` and `finish_resumable_close`:

- check the session cleanup index first and retry it;
- after signal/wait reaches terminal, mark the session record `closed = true`;
- stage `FinalizeSession` with the exact successful close response before the
  host call;
- call `drive_session_cleanup`;
- keep the closed session record plus tombstone on failure; and
- remove the session only after finalization succeeds.

Filter cleanup-pending/closed records from `list_sessions`, and reject prompt or
config mutation on them. `get_session_state` may report the retained closed
state for diagnostics. A repeated `close_session` must find and drive the
tombstone rather than returning the normal absent-session idempotent success.

This also prevents retry from repeating SIGTERM/SIGKILL: only the finalization
action is replayed.

## Aggregate error shape

Add to `AcpCoreError`:

```rust
Cleanup {
    context: &'static str,
    errors: Vec<AcpCoreError>,
}
```

Its code is `cleanup_failed`. Require a non-empty vector, preserve operation
order, and format numbered children without flattening their codes internally.
Update native/browser host-to-core maps to preserve `Cleanup`; do not convert it
to `Execution` or `InvalidState`.

Stable child order for native finalization is:

1. adapter output-route cleanup;
2. terminal IDs in `BTreeMap` order, kill before output cleanup;
3. extension process IDs in `BTreeSet` order;
4. extension VM IDs in `BTreeSet` order; and
5. core-process metadata removal last, only if all prior phases succeeded.

## Native finalization implementation

Rename `NativeCoreCommand::ReleaseAgentRoute` to `FinalizeSessionCleanup` and
carry both IDs. `NativeCoreHost` forwards the new host method.

Refactor `release_native_agent_route` into a finalizer that attempts all three
independent phases even when one fails:

- `stop_buffering_process_output(process_id)`;
- `cleanup_session_terminals(session_id)`; and
- `dispose_session_resources_wire(session_id)`.

Aggregate every failure. Retain `core_processes[(owner, process)]` until all
phases succeed. Each underlying phase must be idempotent.

In `NativeSidecar::dispose_session_resources`, do not remove
`ExtensionSessionResources` up front. Snapshot sorted IDs, attempt all of them,
and remove each ID from the retained record only when that resource succeeded or
is authoritatively already absent. Remove the record when both sets are empty.
An error from one process/VM must not skip later resources.

Browser `finalize_session_cleanup` should drive its existing execution/context
cleanup and retain failed non-routable worker cleanup state as described by the
main Item 36 note; it must be idempotent for a missing completed route.

## Focused test checklist

Replace the vulnerable abort regression at `engine.rs:5805-5847` with assertions
that:

- first abort failure returns `cleanup_failed` and retains one tombstone;
- active pending count is zero and output is rejected;
- wrong-owner retry neither invokes the host nor reveals the tombstone;
- exact-owner retry calls abort again, returns the originally stored terminal
  response, and removes the tombstone; and
- a third retry is the normal unknown-pending error.

Add these shared-core tests:

- cleanup limit `1`: one failed cleanup blocks a new process admission with a
  typed/how-to-raise error; successful retry frees the slot;
- a prompt-abort cleanup retry restores its preamble once and starts at most one
  replacement adapter;
- cleanup retry cannot change terminal response by changing abort reason;
- close finalizer fails once: session is closed/non-routable, absent from list,
  no signal phase repeats, retry calls only finalization, then removes state;
- finalize failure plus a second child failure returns ordered
  `Cleanup { errors: [...] }`; and
- owner disposal includes cleanup tombstones, tries all in sorted process order,
  and never exposes them to another owner.

Update native tests to inject independent failures in output cleanup, terminal
cleanup, extension process cleanup, and extension VM cleanup. Assert all phases
were attempted, only failed handles remain, retry clears them, and
`core_processes` is removed last.

Keep `drop_owner_state` for the narrow case where the embedding host has already
authoritatively destroyed every resource. Ordinary disconnect/dispose must
attempt tombstones and log/return the exact aggregate; it may not silently call
`drop_owner_state` first.

## Suggested validation

```bash
cargo test -p agentos-sidecar-core engine::tests
cargo test -p agentos-sidecar-core
cargo test -p agentos-sidecar acp_extension --lib
cargo test -p agentos-sidecar --test acp_wrapper_conformance -- --nocapture
cargo test -p agentos-native-sidecar extension --lib
cargo check --workspace
git diff --check
```

Keep this work in Item 36's dedicated revision. The shared-core portion should
touch `agentos-sidecar-core/{lib.rs,host.rs,engine.rs}`, native/browser host seam
implementations, and their focused tests; it does not require a client or ACP
wire-schema change.
