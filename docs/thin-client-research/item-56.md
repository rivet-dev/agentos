# Item 56 research — make asynchronous cron dispatch acknowledged

Status: implementation-ready research only. This note does not change production
code, tests, or the Item 56 tracker status.

## Recommendation

Replace `CronDispatchEvent` with a typed sidecar-initiated
`CronDispatchRequest`/`CronDispatchResultResponse` exchange. Give each pending
dispatch a sidecar-owned monotonic cursor, retain the pending FIFO and last
acknowledged cursor in the opaque cron snapshot, and allow only the FIFO head to
have one in-flight reverse request. Make clients cache the result for the last
cursor so retransmission returns the same answer without applying the alarm or
invoking a callback twice. For an actor, persist the opaque pending snapshot as
a write-ahead record before applying host effects and returning the ack.

Priority: **P0**. Confidence: **high** that the current event is lossy and that a
correlated reverse request is the right boundary. Confidence is **medium-high**
on the exact implementation size because the native sidecar already has a
non-blocking sidecar-request queue, while the browser wire dispatcher currently
accepts only host `RequestFrame`s and returns only `EventFrame`s from
`pollEvent`. Worse, the production `ConvergedSidecarHandle` exposes only
`pushFrame`; its factory drops the WASM `pollEvent` method entirely. Browser
reverse-frame delivery and its production pump must therefore land in this
revision or Item 56 must be stacked after Item 73's asynchronous browser frame
boundary. A browser unit test that calls `BrowserWireDispatcher::poll_event_bytes`
directly is not sufficient parity evidence.

Do not solve this by enlarging the Rust control-event log, the TypeScript event
buffer, or the cron lifecycle broadcast. Those remain bounded observer streams
and cannot turn an unacknowledged state transition into reliable delivery.

## Original issue and exact failure path

The tracker entries are at `docs/thin-client-migration.md:102,189,281`.

The scheduler commits the state transition before the host sees it:

1. `CronScheduler::complete` removes the active run, decrements its job's
   running count, may create the queued follow-on run, and returns the new alarm
   and lifecycle records (`crates/native-sidecar-core/src/cron.rs:369-396`).
2. Native exec exit handling performs that completion and then wraps the result
   in a fire-and-forget `CronDispatchEvent`
   (`crates/native-sidecar/src/service.rs::handle_cron_execution_event`, around
   lines 2241-2311). Browser does the same in
   `BrowserWireDispatcher::execution_event_to_frame`, around lines 2350-2412.
3. The Rust transport categorizes the event as ordinary control traffic at
   `crates/sidecar-client/src/transport.rs:920-939`. Its log is deliberately
   bounded to 4,096 entries and roughly one negotiated maximum frame
   (`transport.rs:24-40`), so an older cron dispatch can be evicted.
4. The Rust client consumes the event from the same ACP control pump at
   `crates/client/src/agent_os.rs:639-710`. On lag, Item 22 now fails the route,
   clears the alarm, and stops cron operations (`agent_os.rs:693-733`), but the
   already-committed sidecar completion is not replayed.
5. TypeScript receives the same observer event in
   `packages/core/src/agent-os.ts:2401-2436`. The generic runtime event buffer is
   also bounded (`packages/runtime-core/src/event-buffer.ts:126-169`), and event
   listeners have no acknowledgement (`packages/runtime-core/src/protocol-client.ts:367-390`).

The concrete observable loss today is an updated absolute alarm and completion
record after a sidecar-owned exec exits. The current native and browser exec
paths call `start_cron_runs` before emitting the event, so the `runs` list is
normally empty for an exec job: a queued run has the same serializable exec
action and is launched in the sidecar. The schema nevertheless permits host
callback runs in this event, and both clients execute any such run without an
ack. The tracker wording is therefore correct as a protocol invariant, but the
strongest current regression is the stale-alarm/stranded-completion case, not a
normal exec-to-callback transition.

Synchronous `WakeCron`, `CompleteCronRun`, and `ImportCronState` responses are a
separate cancellation window: the response is correlated, but a caller can
drop its future after the sidecar commits and before `consume_dispatch` runs.
Item 56 should reuse the same cursor consumer for those responses where
practical, but the minimum P0 fix is to remove asynchronous committed state from
the unacknowledged `EventPayload` route.

## Protocol shape

Edit `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:977-1048` and
regenerate both Rust and TypeScript bindings. Replace the event with these
generated wire concepts (field spelling follows the BARE schema):

```text
type CronRunResult struct {
  runId: str
  error: optional<str>
}

type CronDispatchRequest struct {
  cursor: u64
  alarm: CronAlarm
  runs: list<CronRun>
  state: JsonUtf8
}

type CronDispatchResultResponse struct {
  cursor: u64
  runResults: list<CronRunResult>
  error: optional<str>
}

type CronLifecycleEvent struct {
  event: CronEventRecord
}
```

Add `CronDispatchRequest` to `SidecarRequestPayload` and
`CronDispatchResultResponse` to `SidecarResponsePayload`. Remove
`CronDispatchEvent` from `EventPayload`; replace it with the observer-only
`CronLifecycleEvent`. Retaining alarm/runs in an event path would preserve the
ambiguous, lossy route. Lifecycle records are notifications, not scheduler
authority: they may use the existing bounded control-event delivery and surface
a typed lag error without disabling cron. This separation also prevents up to
4,096 bounded 64-KiB error records from making the 8-MiB durable scheduler
snapshot impossible to encode.

`state` is an opaque snapshot produced by the sidecar after the dispatch is
queued. Ordinary clients ignore it. A durable host may store it verbatim before
answering, which lets the actor recover the pending cursor and deferred work
without inspecting scheduling state. This is the only extra host hook needed;
do not teach the actor or either SDK to parse the snapshot.

The request payload for one cursor is immutable. Retransmitting a cursor must
carry byte-equivalent alarm/run/state fields. To preserve that invariant
without adding a second scheduler transaction log, reject schedule, cancel,
wake, and import mutations for that VM with a typed `cron_dispatch_pending`
response until the head is acknowledged. Concurrent run completions must still
commit into later immutable FIFO entries, because up to the active-run bound may
exit while the head is in flight. `list` and `export_state` remain read-only and
may proceed. This is deliberate fail-closed backpressure: silently mutating a
dispatch that a client may already have applied would make cursor deduplication
false. A future optimization can queue mutations inside the scheduler, but must
not weaken the immutable-cursor rule.

The response's top-level `error` means “the host could not apply this dispatch”
(for example its durable state or alarm hook failed). Per-run callback failures
belong in `runResults`. The sidecar must validate the cursor, require exactly
one result for every host callback run and no unknown run IDs, and leave the
dispatch pending on a top-level error or malformed response. The clients must
always return this typed result payload; they must not fail the generic callback
future, because Rust's current `dispatch_sidecar_request` only logs callback
errors and sends no frame.

Generated/compatibility conversion edits are required in
`crates/sidecar-protocol/src/protocol.rs:1030-1138,1445-1464,1847-1878` and
`crates/sidecar-protocol/src/wire.rs:1040-1125`. Item 45 may later delete the
compatibility layer, but Item 56 cannot leave the two protocol representations
divergent. Regenerate `packages/runtime-core/src/generated-protocol.ts` with the
existing `pnpm --dir packages/build-tools build:protocol` path; do not edit that
generated file by hand.

## Sidecar state-machine edits

### Shared scheduler: `crates/native-sidecar-core/src/cron.rs`

Add a bounded pending-delivery state beside `active_runs` around lines 172-223:

- `next_dispatch_cursor: u64`;
- `last_acked_dispatch_cursor: Option<u64>`;
- a FIFO of immutable `PendingCronDispatch` records containing the cursor,
  alarm and host runs; the adapter's one in-flight delivery
  caches the fully built wire request (including its exported opaque snapshot)
  for byte-equivalent retransmission, avoiding a recursive snapshot-inside-
  snapshot representation; and
- explicit deferred-delivery state (or, preferably, leave an overlap-queued job
  queued until ack) so import cannot return pending work as a fresh active run.

The queue must be bounded by `MAX_ACTIVE_CRON_RUNS` (currently 4,096), warn at
80%, and use a typed scheduler error naming the limit. Once any dispatch is
pending, do not start new due/overlap work: concurrent executions already in
flight can contribute at most the active-run bound, while pausing `wake` and
deferring overlap follow-ons prevents an unacknowledged host from creating an
unbounded completion backlog. A later applied alarm may already be due; the
normal host timer will immediately wake it after the queue drains.

Split the current completion operation (`cron.rs:369-396`) into the existing
request/response completion and an asynchronous completion operation that:

1. commits the completed run;
2. records its alarm/host runs under the next cursor and returns lifecycle
   records separately as best-effort observer events;
3. retains an overlap-queued follow-on as queued rather than allocating or
   launching its next run before the cursor is acknowledged; and
4. exposes only the head pending dispatch.

Add scheduler methods with semantics equivalent to:

```rust
complete_async(request, now_ms) -> Result<u64, CronSchedulerError>
pending_dispatch() -> Option<CronDispatchRequestParts>
ack_dispatch(cursor, run_results, now_ms) -> Result<CronDispatchAck, CronSchedulerError>
```

`CronDispatchAck` should distinguish `Applied { runs, events }` from
`Duplicate`; returning the deferred runs again on a duplicate ack would execute
them twice. `ack_dispatch` must be idempotent for exactly the last acknowledged
cursor, reject a future/stale cursor with a typed error, remove the head only
after validating the whole response, apply callback results through the same
completion logic, and return newly released work once for the sidecar adapter
to execute. Never let a client mutate the alarm, job state, overlap policy, or
cursor.

Bump `CRON_STATE_VERSION` at `cron.rs:26` and extend
`CronStateSnapshot`/`export_state`/`import_state` at lines 178-210 and 416-600
with both cursor counters, pending records, and deferred-delivery state. On
import, pending records must be replayed first; deferred sidecar-owned runs must
not also appear in `CronStateImportedResponse.runs`. This closes an existing
sharp edge in which all imported active actions are currently returned
generically at lines 553-599, even when the action is exec and belongs in the
sidecar. Reject snapshots with duplicate/non-monotonic cursors, a last-acked
cursor not below the pending head, unknown run/job references, or a combined
active-plus-pending count above `MAX_ACTIVE_CRON_RUNS`.

Keep the whole serialized snapshot under `MAX_CRON_STATE_BYTES` (8 MiB). Store
run/job IDs in pending records and reconstruct the action from the authoritative
job when possible; cloning a maximum-size action once per active run could
otherwise defeat the snapshot bound.

Add unit coverage in the existing `#[cfg(test)]` module in this file for cursor
allocation, duplicate acknowledgement, future-cursor rejection, paused wakes,
queue bounds, snapshot round-trip, and “pending deferred run is not returned as
a second fresh run on import.”

### Native adapter: `crates/native-sidecar/src/service.rs`

Replace the `CronDispatchEvent` construction in
`handle_cron_execution_event` (currently lines 2241-2311) with a
non-blocking reverse-request state machine:

- correlate `(request_id -> vm_id, cursor)` and one cached in-flight request per
  VM beside `cron_schedulers`/`cron_process_runs` in `NativeSidecar`;
- queue the head through `queue_wire_sidecar_request` (currently line 3742);
- consume matching responses through `accept_wire_sidecar_response` and
  `take_wire_sidecar_response` (currently lines 3824 and 3844);
- on a valid ack, call `ack_dispatch`, pass its deferred work to
  `start_cron_runs` (`service.rs:1598-1710`), and queue the next cursor if one
  exists; and
- on a top-level host error, retain the cursor and retry it with a sidecar-owned
  bounded backoff only after that response proves the prior handler finished.
  Never allocate a second in-flight request for one cursor.

Use this existing queued request machinery rather than the blocking
`SharedSidecarRequestClient::invoke` at `crates/native-sidecar/src/state.rs:221-245`.
The cron handler may await a user callback and a durable actor write; blocking
the sidecar event loop would strand unrelated requests. The stdout writer
already prioritizes sidecar response/control frames, and the reader already
accepts `SidecarResponseFrame`s independently.

`crates/native-sidecar/src/stdio.rs::handle_protocol_frame` is the production
ack continuation point. Immediately after accepting a `SidecarResponseFrame`,
take any matching cron response, apply it, launch the once-released sidecar
work, and flush the next head. The periodic process-event pump must also call a
small `queue_pending_cron_dispatches` helper so restored pending cursors are
emitted even when no new host request arrives. Keep this work non-blocking with
respect to host callbacks; only `start_cron_runs` may await sidecar-owned session
work after the host has already answered the reverse request.

Remove the obsolete `CronDispatchEvent` import and make VM/session disposal
purge the cursor correlation and in-flight delivery exactly where
`cron_schedulers`/`cron_process_runs` are purged in `reclaim_vm_tracking` and the
session/connection cleanup paths. This requires a bounded
`SidecarResponseTracker::abandon_request(request_id)` API in
`crates/sidecar-protocol/src/protocol.rs`, plus removal from
`outbound_sidecar_requests`, `completed_sidecar_responses`, and their order/gauge
state. Without that, disposal leaks a pending tracker slot for every unanswered
cursor. Late responses should then follow the existing benign stale-response
policy.

The native stdout path already gives queued sidecar requests their own bounded
frame route; do not put cron back in `EventFrame`. Queue overflow must preserve
the scheduler's pending cursor and return/log a typed limit error naming
`MAX_OUTBOUND_SIDECAR_REQUESTS`; the next event-pump tick may retry queueing the
same cursor after capacity is available.

Do not invent a dispatch timeout in this item. If a host handler never returns,
the pending FIFO stays bounded and cron pauses, but retrying it concurrently
could invoke a callback twice because the current callback protocol has no
expiry/cancellation acknowledgement. Item 68 is the dependency for adding a
bounded authoritative timeout later. Item 56 may safely retry a returned
top-level error (initial 100 ms, capped at 30 s is sufficient); the client must
not cache error responses, and its effect order must guarantee no callback ran
before that error.

### Browser adapter: `crates/native-sidecar-browser/src/wire_dispatch.rs`

The browser currently hard-rejects any input frame other than `RequestFrame`
at lines 159-167, stores only `pending_events` at lines 87-136, and returns only
`EventFrame` from `poll_event_bytes` at lines 200-210. That is the main
implementation risk.

Extend the dispatcher to:

- accept both `RequestFrame` and `SidecarResponseFrame`;
- keep `pending_events` as the lossy observer queue, but add a separate bounded
  one-head-per-VM reverse-request queue/tracker (bounded by the configured
  `max_vms`, itself defaulted to `DEFAULT_MAX_VMS`); prioritize that queue over
  observer events;
- change `handle_request_bytes` to return an optional immediate frame: a host
  `RequestFrame` returns its `ResponseFrame`, while a host
  `SidecarResponseFrame` is consumed and returns no response;
- rename/broaden `poll_event_bytes` and the WASM `pollEvent` export to
  `poll_frame_bytes`/`pollFrame`, returning either `SidecarRequestFrame` or
  `EventFrame` (the shared JS frame classifier already understands both); and
- apply the matching cursor response before releasing deferred runs.

Then replace the `CronDispatchEvent` built by
`BrowserWireDispatcher::execution_event_to_frame` at lines 2350-2412 with the
shared pending-dispatch request. Keep `MAX_PENDING_REQUEST_EVENTS` for observer
events, but give cron dispatch its own one-per-cursor correlation and the shared
scheduler bound; it must not compete with the lossy observer queue.

Do not leave browser cron on the old event as a temporary exception. Project
guidance requires native/browser and Rust/TypeScript wire behavior to remain in
lockstep.

This Rust/WASM work is not sufficient by itself. Today
`packages/runtime-browser/src/default-sidecar.ts` and
`packages/browser/src/converged-sidecar.ts` return only `pushFrame`; they discard
the WASM poll method, and `PushFrameSidecarTransport` assumes every write is a
host request with an immediate response. Stack Item 56 after Item 73 and use its
asynchronous production frame pump. If Item 56 lands first, include the minimum
equivalent boundary in the same revision: expose `pollFrame`, continuously drain
it on the main thread, dispatch `SidecarRequestFrame` through the shared
`SidecarProtocolClient` handler, and write the resulting
`SidecarResponseFrame` back without expecting an immediate frame. Guest
SharedArrayBuffer syscalls must keep their existing synchronous request/response
`pushFrame` path.

## Client edits

### Rust

In `crates/sidecar-client/src/transport.rs`, add `CronDispatchRequest` to
`sidecar_request_key` at lines 1078-1084. It will then use the already
correlated, prioritized `SidecarRequestFrame` path at lines 941-985 rather than
the bounded control log.

In `crates/client/src/agent_os.rs`:

- add a weak cron router scoped to `AgentOsSidecar` in
  `crates/client/src/sidecar.rs`, keyed by the full
  `(connection_id, session_id, vm_id)` ownership tuple;
- register a `"cron_dispatch"` wire callback unconditionally after the VM's
  `AgentOsInner` exists (the current creation point is lines 385-398);
- route by exact VM ownership to `CronManager`; and
- replace the `CronDispatchEvent` arm in the control pump with an observer-only
  `CronLifecycleEvent` arm that publishes one record to `cron_events()` without
  touching alarms, callbacks, or scheduler operations.

Do not copy `VM_PERMISSION_ROUTERS`' process-global `vm_id`-only key. Separate
sidecar processes reuse `conn-1/session-1/vm-1`, and a new global map would make
reliable cron delivery cross-route between pools. The sidecar-handle-scoped map
is legitimate host transport state, is shared by sibling VMs on exactly one
transport, and can be removed only after confirmed session shutdown.

After the event arm is gone, `fail_control_routes` at lines 716-733 should fail
ACP/session observer streams only. Remove the cron call there and the sticky
`event_route_failure`/`ensure_event_route` gates from
`crates/client/src/cron.rs:212-300,460-560,780-898`. A lagging public
`cron_events()` receiver should still receive its own typed stream error, but it
must not disable schedule/list/cancel or clear a correct alarm: authoritative
cron delivery no longer uses that observer stream.

Refactor `CronManager::consume_dispatch` and `execute_run` at
`cron.rs:496-604` into a handler that returns `Vec<CronRunResult>` instead of
issuing `CompleteCronRunRequest` for reverse-delivered runs. Issuing a nested
host request while the sidecar is waiting for `CronDispatchResultResponse` can
deadlock; callback results must travel in that response. Keep the existing
request/response callback completion path for runs returned directly by
`WakeCronResponse` until all synchronous dispatch responses are cursorized.

Store only `(last_cursor, last_success_response)` in `CronManager`. On the same
cursor, return the cached success without reapplying the alarm, persisting
state, or reinvoking callbacks. A higher cursor proves the
previous response was accepted and replaces the cache. Do not cache a top-level
error: the sidecar is allowed to retry it after the prior handler returns. This
is bounded deduplication, not a client-owned scheduler.

Add a host-only optional opaque-state hook next to `CronAlarmHandler` at
`cron.rs:116-130`, for example `CronStateCommitHandler`. For a new cursor, call
the hooks in this order: (1) persist `request.state`; (2) apply the absolute
alarm; (3) invoke callbacks and collect their per-run results; (4) cache and
return the successful ack. If steps 1 or 2 fail, return a top-level error before
any callback runs. Observer lifecycle records arrive separately through
`CronLifecycleEvent`. Regular clients leave the state hook unset; the actor
supplies durable storage. Re-export the named type from
`crates/client/src/lib.rs:85-88`.

Item 37 has landed. Preserve its result-bearing Rust callback because that is
what populates `CronRunResult.error`; do not reintroduce a unit-returning
callback or stringify a `ClientError` prefix here.

### TypeScript

Update `packages/runtime-core/src/callbacks.ts:14-125` and its generated mapping
tests to represent `cron_dispatch` and `cron_dispatch_result`. The existing
protocol client already dispatches a sidecar request outside the event buffer
and awaits the handler (`packages/runtime-core/src/protocol-client.ts:121-126,
342-365`), so no new TypeScript transport queue is needed on the native stdio
path. Extend `isMatchingSidecarResponsePayload` and
`errorSidecarResponsePayload` too, so a thrown handler becomes a typed
top-level cron result rather than an unhandled switch gap.

In `packages/core/src/agent-os.ts:2804-2847`, add a `cron_dispatch` arm to the
existing VM-scoped sidecar-request handler and replace the old `cron_dispatch`
event arm in `_handleSidecarEvent` with `cron_lifecycle`, which only calls the
public listener conversion/emitter.

In `packages/core/src/cron/cron-manager.ts:193-243`:

- make reverse dispatch consumption async and result-bearing;
- await every callback and return its exact error string in `runResults`;
- do not call `completeCronRun` for reverse-delivered runs;
- persist nothing client-side, apply the alarm before callbacks, and turn an
  alarm-driver exception into a retryable top-level error before callbacks run;
- cache only the last successful cursor/result exactly as in Rust; and
- return a top-level dispatch error when applying the alarm fails.

The TypeScript timer alarm driver is currently synchronous, while Rust's actor
alarm hook is fallible/async. Keep ordinary TS application simple; the reverse
handler can await a `Promise.resolve` wrapper without adding schedule policy.
The TypeScript package-manager default exception is unrelated.

## Actor hook and delivery semantics

`crates/agentos-actor-plugin/src/vm.rs:77-97` is the required host-only bridge:
it turns the sidecar's timestamp/generation into Rivet's `schedule_at`. Preserve
it. Add the opaque state commit handler immediately beside it so
`request.state` is stored with `persistence::save_cron_state` before the actor
calls `schedule_at` or answers the dispatch request. Do not call
`export_cron_state` from inside the callback and do not parse the value. If the
write succeeds but `schedule_at` fails, the sidecar retries the same cursor and
the stored pending snapshot remains recoverable. If `schedule_at` succeeds more
than once around a retry/crash, duplicate actor wake actions are harmless only
because `CronScheduler::wake` validates the opaque alarm generation; document
and test that invariant.

On cold boot, `ensure_vm` already imports the stored opaque state at
`vm.rs:98-119`. The extended snapshot replays its pending cursor. Actor cron
actions currently expose only exec/session, not host callback closures, so a
replayed pending dispatch can re-arm the same opaque generation and then start
deferred work once; it cannot duplicate an in-process callback that vanished
with the old process.

Be precise about the guarantee:

- **Within a live VM/connection:** at-most-once host application under request
  retransmission, using the cursor/result cache; no alarm or run is silently
  dropped.
- **Across a controlled actor cold boot from a stored pending snapshot:** the
  restored sidecar exposes the same cursor, the live client applies it once,
  and stale/duplicate wake actions remain harmless because the scheduler
  validates alarm generation.
- **Across an arbitrary crash before the actor stores the post-transition
  snapshot, or after ack but before the existing lifecycle-event pump persists
  the post-ack snapshot:** strict exactly-once execution is impossible without a
  transactional durable sidecar/actor log. Do not claim otherwise. The design
  narrows the commit point to “persist opaque pending state, then apply host
  effects, then ack” and provides at-least-once recovery for those crash
  windows. The existing lifecycle-event persistence and explicit pre-sleep
  export overwrite the write-ahead snapshot with post-ack state during normal
  actor operation.

## Focused before/after tests

### Before evidence

- Add a focused Rust transport regression beside the event-log tests in
  `crates/sidecar-client/src/transport.rs`: publish a `CronDispatchEvent`, force
  enough same-route control events/bytes to evict it, and assert
  `WireEventRecvError::Lagged`. This documents the exact current delivery
  primitive; remove or rewrite it after the event type is deleted.
- Extend `browser_sidecar_executes_cron_commands_and_emits_completion_dispatch`
  in `crates/native-sidecar-browser/tests/wire_dispatch.rs`: poll and
  discard the sole `CronDispatchEvent`, then prove there is no request/cursor
  with which the host can recover its completion or alarm.
- Add a scheduler test in `crates/native-sidecar-core/src/cron.rs` showing that
  `complete` removes the active run before its returned dispatch is consumed.

These are sidecar/transport tests, not SDK policy tests; the before behavior
belongs at the layer that creates and loses the event.

### After: shared sidecar and protocol

- `crates/native-sidecar-core/src/cron.rs`: cursor/ack, duplicate ack, future
  cursor, paused wake, bound, and opaque snapshot replay tests described above.
- `crates/sidecar-protocol` wire round-trips: exact request/response variants,
  cursor, optional errors, and opaque state.
- `crates/native-sidecar/tests/bidirectional_frames.rs` (or a focused new cron
  integration module): return a top-level host error for the first request,
  receive the retried same cursor after backoff, then ack it and assert one
  completion, one callback result, and the final alarm. Also saturate ordinary
  control events and prove the cron reverse request still arrives.
- Rewrite the named browser completion test to receive a
  `SidecarRequestFrame`, answer it with the matching cursor, and assert the
  completion is applied exactly once. Add duplicate-response, retransmitted
  request, outbound-frame bound, and dispose-with-in-flight-request cases.

### After: Rust and TypeScript clients

- `crates/client/src/cron.rs` unit tests: deliver one cursor twice and assert the
  alarm and callback each run once while both calls return byte-equivalent
  results; separately deliver `CronLifecycleEvent` twice and prove it affects
  only the public observer stream, never the alarm or callback. Test a failed
  state/alarm hook returns a top-level error, runs no callback, is not cached,
  and succeeds exactly once on retry. Add two sidecar handles whose full wire
  ownership strings both equal `conn-1/session-1/vm-1` and prove their cron
  routers cannot cross.
- `crates/client/tests/cron_e2e.rs`: keep the existing real callback test, add a
  forced retransmission hook/test transport where feasible, and assert the
  callback count and sidecar `run_count` are exactly one.
- `packages/runtime-core/tests/callbacks.test.ts` and
  `protocol-frames.test.ts`: generated/live cron request and result mappings.
- `packages/core/tests/cron-manager.test.ts`: same-cursor replay invokes the
  callback once and returns the cached `runResults`; callback rejection becomes
  the exact per-run error; top-level alarm failure is retryable.
- `packages/core/tests/cron-integration.test.ts`: retain callback and exec E2Es,
  asserting one completion and `running === false`; add a protocol-level
  retransmission case rather than trying to overflow a public listener.

The old client tests that merely consume `CronDispatchEvent` should be deleted
or rewritten as reverse-request tests. Do not preserve a client-side event-loss
test after the authoritative behavior moves to the sidecar request path.

After changing the union, run
`rg -n "CronDispatchEvent|cron_dispatch" crates packages tests` and classify
every hit. Remove the event codec/shape from
`packages/runtime-core/src/event-buffer.ts` and its event-buffer fixtures, plus
the now-impossible exhaustive event arms in Rust process/shell/native tests.
Keep `SidecarCronDispatch` in `packages/runtime-core/src/sidecar-process.ts` only
for the still-synchronous `wakeCron`, `completeCronRun`, and `importCronState`
responses; do not accidentally delete those request/response paths while
removing the event variant.

### After: actor

Stack after Item 40 so the real sidecar prerequisite cannot silently skip.
Extend `actor_cold_boot_restores_sidecar_owned_cron_state` at
`crates/agentos-actor-plugin/src/persistence_e2e.rs:529-596` with a pending
dispatch snapshot: persist before ack, tear down the first VM, restore, answer
the replayed cursor, and assert the deferred command/run count changes once.
Also assert the actor schedules the generation once per live cursor and that a
duplicate same-process request is answered from cache.

### Deterministic validation commands

Run the focused checks in this order; every filter below should correspond to a
named regression added above, not a timing-only sleep:

```sh
pnpm --dir packages/build-tools build:protocol
cargo test -p agentos-sidecar-protocol cron_dispatch
cargo test -p agentos-native-sidecar-core cron_dispatch
cargo test -p agentos-sidecar-client cron_dispatch
cargo test -p agentos-native-sidecar --test bidirectional_frames cron_dispatch
cargo test -p agentos-native-sidecar --test service cron_dispatch
cargo test -p agentos-native-sidecar-browser --test wire_dispatch cron_dispatch
cargo test -p agentos-client cron_dispatch
pnpm --dir packages/runtime-core exec vitest run tests/callbacks.test.ts tests/protocol-frames.test.ts tests/protocol-client.test.ts
pnpm --dir packages/core exec vitest run tests/cron-manager.test.ts tests/cron-integration.test.ts
pnpm --dir packages/browser exec vitest run tests/runtime-driver
cargo build -p agentos-sidecar
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" cargo test -p agentos-actor-plugin actor_cold_boot_restores_sidecar_owned_cron_state -- --nocapture
cargo check --workspace
pnpm check-types
cargo fmt --all --check
```

The before characterization is the `agentos-sidecar-client` lag test plus the
current browser completion-event test. Record both passing against the parent
revision. After the protocol variant is removed, replace them with the named
reverse-request tests and record those passing in the Item 56 tracker checklist;
do not mark the item complete from compilation or scheduler unit tests alone.

## Dependencies and risks

- **Item 22:** remove cron from the shared ACP control-route failure path only
  after `CronDispatchEvent` is gone. Today `spawn_acp_event_pump` turns control
  log lag into `fail_control_routes`, `CronManager::fail_event_route` clears the
  host alarm, and `ensure_cron_event_route` rejects every later cron operation.
  After Item 56, that failure remains terminal for ACP/public observer streams
  but cannot clear or disable the independently acknowledged cron route.
- **Item 37:** already landed; preserve its result-bearing Rust callback when
  populating `CronRunResult.error`.
- **Item 40:** already landed; its non-skippable real sidecar cold-boot test is
  the required actor proof.
- **Item 64:** independent semantic schedule codes; avoid touching cron rejection
  normalization in this revision.
- **Item 68:** not required for reliable completed responses. It is required
  before adding an authoritative timeout/retry for a host handler that never
  returns; do not race two handlers for the same cursor in Item 56.
- **Item 73 / browser frame plumbing:** highest implementation risk and preferred
  parent. Validate the public runtime/browser pump, not only the Rust dispatcher;
  a browser-only fallback event is not acceptable.
- **Snapshot size:** adding pending state can expose the existing 8 MiB cap.
  Avoid action duplication and extend the limits inventory fixture if a new
  pending-dispatch bound is introduced.

## Bounded dedicated `jj` revision

Create one dedicated stacked revision for Item 56 after the already-landed
Items 37 and 40, preferably after Item 73 so its browser async frame pump can be
reused. Expected paths:

```text
crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare
crates/sidecar-protocol/src/protocol.rs
crates/sidecar-protocol/src/wire.rs
packages/runtime-core/src/generated-protocol.ts
packages/runtime-core/src/callbacks.ts
packages/runtime-core/src/event-buffer.ts
packages/runtime-core/src/sidecar-process.ts
packages/runtime-core/tests/callbacks.test.ts
packages/runtime-core/tests/event-buffer.test.ts
packages/runtime-core/tests/protocol-frames.test.ts
crates/native-sidecar-core/src/cron.rs
crates/native-sidecar/src/service.rs
crates/native-sidecar/src/stdio.rs
crates/native-sidecar/tests/bidirectional_frames.rs
crates/native-sidecar-browser/src/wire_dispatch.rs
crates/native-sidecar-browser/src/wasm.rs
crates/native-sidecar-browser/tests/wire_dispatch.rs
crates/sidecar-client/src/transport.rs
crates/client/src/agent_os.rs
crates/client/src/cron.rs
crates/client/src/sidecar.rs
crates/client/src/lib.rs
crates/client/tests/cron_e2e.rs
packages/core/src/agent-os.ts
packages/core/src/cron/cron-manager.ts
packages/core/tests/cron-manager.test.ts
packages/core/tests/cron-integration.test.ts
packages/runtime-browser/src/runtime-driver.ts      # omit only when Item 73 already supplies the pump
packages/runtime-browser/src/default-sidecar.ts     # same dependency rule
packages/browser/src/converged-sidecar.ts           # same dependency rule
packages/browser/tests/runtime-driver/*             # production frame-pump regression
crates/agentos-actor-plugin/src/vm.rs
crates/agentos-actor-plugin/src/persistence_e2e.rs
crates/native-sidecar/tests/fixtures/limits-inventory.json
docs/thin-client-migration.md
```

Do not combine Item 56 with schedule-error normalization, ACP event cleanup,
generic process routing, or callback expiry. Describe the revision as
`fix(cron): acknowledge asynchronous sidecar dispatch` and mark the tracker row
done only after before evidence and all native/browser/Rust/TypeScript/actor
checks are recorded.
