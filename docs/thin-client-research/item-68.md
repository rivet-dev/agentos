# Item 68 research — terminate host callback routes from the sidecar

Status: implementation-ready research only. This note does not modify
production code, tests, generated files, or the Item 68 tracker status.

## Recommendation

Add one generic, sidecar-written terminal protocol frame correlated by the
existing sidecar-request ID:

```bare
type SidecarRequestCancellationReason enum {
  DEADLINE_EXCEEDED
  CANCELLED
}

type SidecarRequestCancelledFrame struct {
  schema: ProtocolSchema
  requestId: RequestId
  ownership: OwnershipScope
  reason: SidecarRequestCancellationReason
}
```

Append `SidecarRequestCancelledFrame` to `ProtocolFrame`. The native sidecar
must emit it only when its timeout/cancellation path atomically removes the
matching callback waiter before a response wins. It is a one-way terminal
control frame, not another request requiring acknowledgement.

Both host transports should correlate the frame to the active inbound callback:

- TypeScript aborts an `AbortSignal` supplied to the registered
  `LiveSidecarRequestHandler` and suppresses any response whose generation has
  not already entered the transport writer;
- Rust resolves a cancellation context supplied to `WireSidecarCallback`,
  removes the callback route, and applies the same suppression rule; and
- transport shutdown terminates the same contexts locally.

Then remove `cleanupAfterMs` from `AcpPermissionCallback` and delete the
TypeScript and Rust client timers. Permission callbacks wait for only an
explicit host reply, session/transport shutdown, or the correlated sidecar
cancellation signal. The sidecar remains the only owner of the 120-second
permission deadline and default reply.

Priority: **P2**. Confidence: **medium-high**. The missing signal and both
client timers are explicit. The remaining uncertainty is implementation/race
surface across the generic bidirectional frame transports, not ownership or
desired behavior.

## Why this belongs in the generic callback protocol

The timing authority and the route owner currently sit on opposite sides:

```text
ACP adapter asks permission
          |
          v
native sidecar emits SidecarRequest(-N) and starts authoritative 120s wait
          |
          v
TS/Rust client installs host reply correlation and starts a separate 125s timer
          |
          +---- sidecar reaches 120s, removes waiter, applies default reject
          |
          +---- client cannot observe that; route survives for 5s
                    and may write a response the sidecar can only discard
```

The sidecar cannot remove a JavaScript callback or Rust reply sender directly.
The clients cannot infer the authoritative timeout from a duplicated duration
without recreating policy. The protocol must carry the sidecar's actual terminal
decision.

An ACP-only `permission_expired` event would work for this one route but would
duplicate correlation in every product client and leave host-tool, JS bridge,
and future extension callbacks with the same transport gap. A deadline field on
`SidecarRequestFrame` would still make clients run policy timers and would be
subject to clock/scheduling skew. A generic terminal frame uses the request ID
the transports already own and keeps ACP clients ignorant of timeout policy.

Do not add a cancellation request that itself needs a response. Once the
sidecar has removed its waiter, waiting for an acknowledgement would create a
second failure/timeout cycle. Ordered delivery on sidecar stdout is enough to
tell the trusted, same-version host transport to drop its corresponding route.

## Exact current behavior

### Native sidecar owns the real deadline

`crates/agentos-sidecar/src/acp_extension.rs` defines:

```rust
const PERMISSION_CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const PERMISSION_CALLBACK_CLEANUP_GRACE: Duration = Duration::from_secs(5);
```

`build_inbound_response` around current lines 1540-1600 encodes an
`AcpPermissionCallback` with `cleanup_after_ms = 125_000`, registers an
`ExtensionCallbackCancellation`, and calls
`ExtensionSnapshot::invoke_callback_cancellable` with only the authoritative
120-second timeout.

`permission_callback_reply_from_result` around current lines 2427-2442 maps
`SidecarError::Timeout` to the sidecar default (`"reject"`). The sidecar does
not wait for the extra five seconds and does not receive any signal from the
client to decide policy.

### The native callback waiter already has the correct race point

`FrameSidecarRequestTransport::send_request_inner` in
`crates/native-sidecar/src/stdio.rs:1735-1847` owns a pending map keyed by the
negative sidecar request ID. `accept_response` removes the same entry before it
sends the response to the waiter.

On explicit extension cancellation, `send_request_inner` also removes that
entry and returns `"extension callback was cancelled"`. On timeout it removes
the entry and returns `SidecarError::Timeout`. A response received afterward is
unmatched: the stdin reader cannot claim it through `accept_response`, so it is
forwarded to the normal sidecar path and treated as a stale response.

This pending-map removal is the linearization point. If removal returns the
sender, timeout/cancellation won and the sidecar must emit the cancellation
frame. If removal finds no sender, `accept_response` already won; the waiter
must receive that response and no cancellation frame may be emitted.

There is one distributed race that the implementation and tests must describe
honestly. Sidecar stdout (request/cancellation) and host stdin (response) are
separate byte streams. A host may therefore enqueue a response just before the
sidecar removes the waiter, yet the sidecar may remove the waiter before its
stdin reader accepts those response bytes. A one-way cancellation frame cannot
retract that already-written frame and an unknown cancellation ID at the host
does **not** prove that the response won in the sidecar. The current fallback
through `NativeSidecar::accept_sidecar_response` safely classifies unmatched
responses as benign stale replies, so this race does not corrupt another route
or terminate the shared sidecar.

Item 68 should guarantee that no reply is generated after the client observes
the terminal frame, and that both sides terminate their correlation once. It
must not claim that a frame already accepted by the host's writer can never
lose the authoritative sidecar race. If product requirements demand positive
proof that every submitted response was accepted, that needs a second
sidecar-to-host `accepted` acknowledgement (a larger protocol change), not a
client timer.

The existing tests
`cancellable_sidecar_callback_wait_stops_without_waiting_for_its_deadline` and
`callback_response_and_cancellation_complete_wait_exactly_once` in
`crates/native-sidecar/src/stdio.rs` already cover the sidecar half of this
race. They currently prove that a deliberately late response is rejected, but
there is no frame telling the host not to send it.

### TypeScript manufactures a later cleanup timer

The generated ACP field is declared in
`crates/agentos-protocol/protocol/agent_os_acp_v1.bare:270-278` and generated
into `packages/core/src/sidecar/agentos-protocol.ts`.

`AgentOs._handleAcpExtSidecarRequest` in
`packages/core/src/agent-os.ts:2850-2905` checks that `cleanupAfterMs` fits the
JavaScript number range and passes it to `_handleAcpPermissionCallback`.

`_handleAcpPermissionCallback` at current lines 2908-2961 installs a
`setTimeout(cleanupAfterMs)`. Until it fires, the entry remains in
`AgentSessionEntry.pendingPermissionReplies`, so `respondPermission` can still
find a route after the sidecar has already selected its default. When the timer
finally rejects, the function logs and returns `undefined`; the protocol client
then writes an `ext_result` for a sidecar request whose waiter is gone.

`packages/core/tests/session-config-routing.test.ts` currently codifies this:
`uses only the sidecar-owned post-decision cleanup deadline` advances a local
20 ms timer and proves the host route exists until that timer fires. Despite its
name, this is still a client timer over a sidecar-supplied duration.

### Rust independently implements the same timer

`PermissionRouteRequest` in `crates/client/src/session.rs:35-42` carries
`cleanup_after_ms`. `wait_for_permission_reply` at current lines 423-444 races
the pending reply and `PermissionResponder` against
`tokio::time::sleep(cleanup_after_ms)`.

`AgentOs::deliver_sidecar_permission_request` at current lines 1698-1783 removes
the map entry only after `PendingPermissionOutcome::CleanupElapsed`. The unit
test `permission_reply_wait_uses_only_the_later_cleanup_deadline` proves that
local timer behavior.

The callback decoder in `crates/client/src/agent_os.rs:1258-1320` simply copies
the generated field into `PermissionRouteRequest`. This is behavioral parity by
duplicated client machinery rather than one sidecar-owned outcome.

## Exact protocol and sidecar edits

### Sidecar wire schema

In
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare`, add
`SidecarRequestCancellationReason` and `SidecarRequestCancelledFrame` adjacent
to `SidecarRequestFrame`/`SidecarResponseFrame`, then append the new frame to
`ProtocolFrame`.

Use only two wire reasons:

- `DEADLINE_EXCEEDED`: the sidecar's request timeout won;
- `CANCELLED`: a sidecar lifecycle operation, such as ACP turn/session
  cancellation, ended the callback wait.

Transport disconnect is not written on this wire because there is no connection
on which to deliver it. Each client should terminate active contexts with its
existing local transport error instead.

Regenerate `packages/runtime-core/src/generated-protocol.ts` with:

```sh
pnpm --dir packages/build-tools build:protocol
```

Do not hand-edit the generated file and do not bump committed product versions.

### Rust protocol compatibility layer

Update `crates/sidecar-protocol/src/protocol.rs`:

- add `ProtocolFrame::SidecarRequestCancelled`;
- add the compat `SidecarRequestCancellationReason` and
  `SidecarRequestCancelledFrame` types (or direct generated aliases consistent
  with the existing wire migration style);
- map the new generated frame in both protocol conversion directions; and
- include it in serde/JSON protocol-frame coverage.

Update `crates/sidecar-protocol/src/wire.rs` so frame validation accepts the new
sidecar-written control frame, validates its schema, nonzero request ID, and
ownership, and rejects it where a host-written request/response frame is
required. Do not register it with `SidecarResponseTracker`: it terminates the
sidecar's stdio waiter, not the compatibility service's queued response object.

### Native stdio callback transport

In `crates/native-sidecar/src/stdio.rs`, add a small helper on
`FrameSidecarRequestTransport` that:

1. removes `request.request_id` from `pending` under the existing lock;
2. if the route was present, emits
   `ProtocolFrame::SidecarRequestCancelledFrame` with the original ownership
   and selected reason through the existing bounded stdout writer;
3. returns the authoritative timeout/cancellation error; and
4. if the route was absent, waits for the already-claimed response sender and
   returns that response instead.

Use the helper from both cancellation checks after the request frame has been
successfully enqueued and from the response-deadline branch. If cancellation or
timeout happens before the request frame was written, remove the local waiter
but emit no cancellation frame because the host never received a route.

Do not ignore cancellation-frame write failure. Propagate it as the existing
bridge/I/O failure so the host-visible transport path reports that terminal
correlation could not be delivered.

The stdio transport's `pending` map is currently not protected by the service
tracker's 10,000-entry admission check. Bound it in this revision before making
host transports depend on it: reuse `MAX_PENDING_SIDECAR_RESPONSES`, make the
existing `sidecar_response_pending_overflow_error` available within the crate,
and have `FrameSidecarRequestTransport` observe the existing
`pending_sidecar_responses_gauge` on insert, response, cancellation, and write
failure. Pass that gauge from the `NativeSidecar` instance when constructing
the stdio callback transport. This reuses the sidecar-owned limit and its
near-threshold warning instead of copying a limit into each client.

Update exhaustive frame-kind/host-input matches in `stdio.rs`. A
`SidecarRequestCancelledFrame` received from host stdin is invalid direction;
the native sidecar only writes this frame.

No policy change belongs in `permission_callback_reply_from_result`: timeout
still maps to the ACP default and explicit lifecycle cancellation retains its
current enclosing-operation behavior.

## Exact TypeScript transport edits

### Live frame model and codec

In `packages/runtime-core/src/protocol-frames.ts`:

- add `LiveSidecarRequestCancelledFrame` with
  `frame_type: "sidecar_request_cancelled"`, request ID, ownership, and the two
  lowercase live reasons;
- include it in `LiveSidecarWrittenProtocolFrame` and the general live frame
  union used by JSON tests;
- decode the generated frame and map the generated reason without
  interpreting it; and
- classify it as a distinct control kind.

In `packages/runtime-core/src/frame-rpc.ts`, extend `ClassifiedFrame` and
`FrameRpcTransport` with a sidecar-request-cancelled type/listener. Do not send
the frame through the normal event buffer; it is transport correlation, not a
public VM event.

Add a protocol transport error in
`packages/runtime-core/src/sidecar-errors.ts`, for example
`SidecarRequestCancelled`, retaining request ID, ownership, and exact reason.
Use it as `AbortSignal.reason`; do not translate `DEADLINE_EXCEEDED` into ACP
policy or a permission answer.

### Active inbound callback routes

In `packages/runtime-core/src/protocol-client.ts`, add a map keyed by the
sidecar request ID whose value retains original ownership and an
`AbortController`. Its population is transitively bounded by the trusted
sidecar's `MAX_PENDING_SIDECAR_RESPONSES` admission limit (10,000) after the
stdio bound above is applied; duplicate live IDs are protocol errors, not
replacement opportunities.

Extend `LiveSidecarRequestHandler` in `protocol-frames.ts` to receive a second
context argument:

```ts
export interface LiveSidecarRequestContext {
	readonly signal: AbortSignal;
}

export type LiveSidecarRequestHandler = (
	request: LiveSidecarRequestFrame,
	context: LiveSidecarRequestContext,
) => Promise<LiveSidecarResponsePayload> | LiveSidecarResponsePayload;
```

Existing one-argument functions remain assignable in TypeScript. Pass the
context through `resolveSidecarRequestFramePayload`.

`dispatchSidecarRequest` must:

1. reject a duplicate active request ID;
2. install the route before invoking user/host code;
3. await the handler with its signal;
4. claim/remove the route before writing a response; and
5. suppress the response if a cancellation frame already removed that exact
   route.

The cancellation listener must verify exact ownership for a live ID, remove
the route once, and abort with `SidecarRequestCancelled`. An unknown ID is a
benign terminal race: the response may have won in the sidecar, or it may
already have entered the host writer and then lost to expiry before the
sidecar's stdin reader accepted it. Never interpret unknown as proof of
sidecar acceptance. Transport failure/disposal must abort every active
controller with the existing terminal error and clear the map so handler
promises do not retain correlation indefinitely.

Do not add a timeout to `SidecarProtocolClient`; it consumes the sidecar's
terminal signal.

## Exact Rust transport edits

In `crates/sidecar-client/src/transport.rs`:

- add `WireSidecarRequestContext`, carrying the request ID and a clonable
  cancellation receiver/future;
- change `WireSidecarCallback` to receive `(payload, ownership, context)`;
- add an active inbound-callback map keyed by negative request ID with exact
  ownership plus its cancellation sender;
- insert the route before spawning the callback;
- on callback completion, remove/claim the route before sending
  `SidecarResponseFrame`;
- on `SidecarRequestCancelledFrame`, remove the exact route, verify ownership,
  and resolve its context with the exact reason; and
- on reader failure/silence shutdown, terminate every active context before
  clearing it.

If cancellation won before callback output entered the writer, a callback
future may finish cooperatively but its task must not send a response. Unknown
cancellation IDs are benign terminal races, not necessarily response wins. Do
not publish cancellation through `control_event_log`; permission and other
callback routes consume it directly.

Update the three callback factories in `crates/client/src/agent_os.rs`
(`js_bridge_call_callback`, `permission_request_callback`, and
`host_callback_callback`) for the context parameter. Only the permission path
needs to await cancellation in Item 68. Passing a context to the other paths is
the generic transport contract and allows later host APIs to support
cooperative cancellation without another wire change; do not redesign their
public callback APIs here.

Re-export the context beside `WireSidecarCallback` from
`crates/client/src/transport.rs` if the current private transport import path
requires it. No new Tokio utility dependency is needed; the crate already uses
Tokio channels.

## Remove the ACP cleanup duration

### Shared ACP callback schema

Delete `cleanupAfterMs` from `AcpPermissionCallback` in
`crates/agentos-protocol/protocol/agent_os_acp_v1.bare`. Its payload becomes
only:

```bare
type AcpPermissionCallback struct {
  sessionId: str
  permissionId: str
  params: JsonUtf8
}
```

Regenerate `packages/core/src/sidecar/agentos-protocol.ts` with:

```sh
pnpm --dir packages/core build:agentos-protocol
```

Rust generated ACP types are built from the schema; update all struct literals
rather than adding a replacement duration field.

### Native ACP adapter

In `crates/agentos-sidecar/src/acp_extension.rs`:

- delete `PERMISSION_CALLBACK_CLEANUP_GRACE`;
- stop serializing `cleanup_after_ms` in `build_inbound_response`; and
- remove the 125,000/greater-than-120,000 assertions in the two callback
  handlers in `crates/agentos-sidecar/tests/acp_extension.rs`.

Keep `PERMISSION_CALLBACK_TIMEOUT`, the cancellable wait registry, timeout
logging, and sidecar default mapping. The item removes a client bookkeeping
duration, not the authoritative deadline.

### TypeScript AgentOS client

Stack this item after Item 67 because both edit
`AgentOs._handleAcpPermissionCallback`. Preserve Item 67's explicit fail-fast
handler delivery and immediate local-failure cleanup.

In `packages/core/src/agent-os.ts`:

1. remove `cleanupTimer` from `AgentSessionEntry.pendingPermissionReplies`;
2. remove timer clearing from session-close/dispose and `respondPermission`;
3. have the registered sidecar request handler pass its
   `LiveSidecarRequestContext.signal` through `_handleAcpExtSidecarRequest` to
   `_handleAcpPermissionCallback`;
4. delete the `cleanupAfterMs` safe-integer check and method parameters;
5. register one abort listener for the exact pending entry; when it fires,
   remove that entry and settle the local callback as `undefined` without
   selecting a permission reply; and
6. remove the abort listener in `finally` on explicit reply, handler failure,
   session close, or cancellation.

Use identity when deleting from the map so an old cancellation cannot remove a
new route that reused the same permission ID. Check `signal.aborted` before and
immediately after listener installation so a cancellation that arrived during
callback decoding cannot be missed. Normal sidecar timeout is expected
control flow and should not create a second client warning; the sidecar already
logs that it applied its default.

After Item 67, handler invocation must remain outside the promise constructor.
Its synchronous-failure branch should delete the route and return `undefined`,
but it no longer has a timer to clear. Do not reintroduce incidental
promise-executor exception behavior while wiring the abort signal.

Update fixture shapes in:

- `packages/core/tests/agent-os-dispose-retry.test.ts`;
- `packages/core/tests/session-config-routing.test.ts`;
- `packages/core/tests/permission-no-handler-warning.test.ts` where its helper
  signature names the duration;
- `packages/core/tests/cross-session-permission-reply.test.ts`; and
- Item 67's `permission-handler-failure.test.ts` if that revision has landed.

Some fixtures use their own structural pending-reply type rather than the
production interface. Remove only timer-specific fields/assertions; retain
session isolation, close rejection, and exact reply ownership tests.

### Rust AgentOS client

In `crates/client/src/session.rs`:

- replace `PermissionRouteRequest.cleanup_after_ms` with the generic wire
  callback cancellation context;
- replace `PendingPermissionOutcome::CleanupElapsed` with `Cancelled`;
- have `wait_for_permission_reply` select the pending reply, responder reply,
  or the context's cancellation future—never a sleep;
- on cancellation, remove the exact pending sender and return no reply; and
- update `PermissionRequest` documentation to say the route ends from the
  sidecar signal.

In `crates/client/src/agent_os.rs`, pass the `WireSidecarRequestContext` from
`permission_request_callback` through `handle_acp_ext_callback` and
`route_permission_request`. Remove `callback.cleanup_after_ms` and the field in
the malformed-callback test struct literal around current line 2257.

Do not add a Rust duration constant, sleep, or cancellation-to-`Reject` mapping.
The Rust client returns `reply: None`; the sidecar alone converts its own timeout
to the default.

## Before and after tests

### Before evidence

Use the existing tests as the vulnerable-parent characterization:

- `packages/core/tests/session-config-routing.test.ts` —
  `uses only the sidecar-owned post-decision cleanup deadline` proves the TS
  route stays live until its client timer;
- `crates/client/src/session.rs` —
  `permission_reply_wait_uses_only_the_later_cleanup_deadline` proves Rust does
  the same;
- `crates/native-sidecar/src/stdio.rs` —
  `cancellable_sidecar_callback_wait_stops_without_waiting_for_its_deadline`
  proves the sidecar waiter is already gone while no terminal frame is emitted;
  and
- `callback_response_and_cancellation_complete_wait_exactly_once` proves a
  deliberately late response is rejected after cancellation wins.

Extend the native test on the parent just enough to record that the stdout
channel contains the original `SidecarRequestFrame` and no second terminal
frame after cancellation. That is the exact missing protocol behavior.

### Protocol codec tests

In `crates/sidecar-protocol`, round-trip both cancellation reasons and assert
request ID/ownership preservation in BARE and JSON compatibility tests.

In `packages/runtime-core/tests/protocol-frames.test.ts`, round-trip/decode a
`sidecar_request_cancelled` frame and assert exact live reason mapping. Extend
`packages/runtime-core/tests/frame-rpc.test.ts` so the new classified kind
reaches only its cancellation listeners and counts as frame activity.

### Native sidecar race tests

Extend the existing stdio tests instead of adding a 120-second ACP test:

1. a short callback timeout emits exactly one cancellation frame with the same
   request ID/ownership and `DEADLINE_EXCEEDED`;
2. `ExtensionCallbackCancellation::cancel()` emits exactly one frame with
   `CANCELLED` without waiting for the deadline;
3. when `accept_response` wins, no cancellation frame is emitted; and
4. when cancellation wins, a late response cannot claim the waiter.

This tests the same transport used by the 120-second permission callback while
keeping the default suite fast.

### TypeScript runtime transport tests

In `packages/runtime-core/tests/protocol-client.test.ts`:

- register a handler that records its context and waits for `signal.abort`;
- inject a sidecar request followed by a matching cancellation frame;
- assert `signal.reason` is the structured cancellation error with exact ID,
  ownership, and reason;
- assert the active route is removed and no `sidecar_response` is written even
  if the handler later resolves; and
- cover the opposite race: a completed handler writes one response and a later
  cancellation does not abort a new/reused route or write a second frame.

Add ownership-mismatch and transport-dispose cases. Update the explicit
sidecar-written frame unions in
`packages/runtime-core/tests/shared-sidecar-ownership.test.ts`, and add an
ownership-routing assertion proving cancellation for VM A cannot terminate VM
B's live callback. Do not use a timeout in these tests; direct frame injection
proves the client consumes sidecar state.

### Rust transport tests

In the `crates/sidecar-client/src/transport.rs` test module:

- register a callback that waits on its `WireSidecarRequestContext`;
- feed a sidecar request and then a cancellation frame through
  `handle_wire_frame`;
- assert exact reason, zero active inbound routes, and no control response;
- assert a response-completion winner sends exactly one response and a later
  cancellation is harmless; and
- assert `fail_all_pending` resolves active callback contexts.

Retain the existing bounded writer and pending host-request tests.

### Product-client permission tests

Rewrite the TypeScript fake-timer test in
`packages/core/tests/session-config-routing.test.ts` to pass an `AbortSignal`:

- before abort, the permission route is pending;
- sidecar cancellation immediately removes it;
- `respondPermission` then reports `Permission request is not pending`;
- the callback returns `undefined`; and
- `vi.getTimerCount()` remains zero throughout.

Keep Item 67's synchronous handler-failure tests green and update them to prove
there is no delayed timer side effect.

Replace Rust's cleanup-deadline test with
`permission_reply_wait_ends_on_sidecar_cancellation`. Trigger the context
without sleeping and assert the pending sender is removed, the retained
`PermissionResponder` observes its receiver close, and a late reply cannot be
bridged.

Retain `missing_client_permission_reply_uses_sidecar_default` and
`only_callback_timeout_uses_sidecar_permission_default` in
`crates/agentos-sidecar/src/acp_extension.rs`; they prove cancellation of host
bookkeeping does not move default policy into either client.

## Risks and dependencies

- **Exact race ownership is essential.** Timeout/cancellation may emit a frame
  only after it successfully removes the pending waiter. Response-win must emit
  no cancellation. Test both orders repeatedly or with explicit barriers.
- **Do not overstate the cross-stream guarantee.** Once response bytes enter a
  host writer they cannot be retracted by a later stdout cancellation frame.
  The required guarantee is no newly generated response after observed
  cancellation plus benign handling of an already-in-flight loser. A positive
  response-acceptance guarantee would require a separate acknowledgement.
- **Do not buffer cancellation as an event.** Callback correlation must not
  compete with bounded user event history or event subscribers.
- **Do not reuse request IDs while active.** The sidecar already allocates
  decreasing negative IDs. Clients should reject a duplicate active ID and use
  entry identity when removing routes.
- **Cancellation is not a permission answer.** Both clients return no reply;
  only the sidecar timeout path chooses the ACP default.
- **Transport shutdown must drain routes.** Adding active callback maps without
  aborting them in failure/dispose paths would replace a five-second leak with
  an unbounded one.
- **The active maps must be bounded by the sidecar.** The compatibility service
  already rejects more than 10,000 pending sidecar responses, but the live
  `FrameSidecarRequestTransport` does not currently apply that check. Extend the
  same sidecar-owned bound/gauge to the live transport, then keep both host maps
  one-for-one with emitted requests and retain no completed tombstones.
- **Item 67 should land first.** Item 68 removes its timer but must preserve its
  fail-fast handler contract and local exception cleanup. Resolve the shared
  `agent-os.ts` test edits in the Item 68 revision rather than recombining the
  work.
- **Item 52 is adjacent.** It removes false permission detection from session
  notifications but retains the typed reverse callback. Land it before this
  item if it still edits the callback decoder area.
- **Item 56 may reuse the generic primitive.** Reliable cron callback delivery
  can consume the cancellation context later, but cursor/ack semantics remain
  Item 56. Do not add cron frames or state here.
- **Other host callbacks are not redesigned.** JS bridge/tool callback APIs may
  ignore the new signal initially; transport response suppression still works.
  Cooperative abortion of arbitrary user work needs a separate public API
  decision.
- **No browser-adapter policy fork.** `agentos-native-sidecar-browser` does not
  currently originate this native ACP permission callback. It needs no timeout
  implementation. The shared generated frame and runtime-core transport remain
  usable by injected/browser transports without an ACP-specific branch.
- **No new dependency is needed.** Use existing `AbortController` and Tokio
  channel primitives; avoid adding a cancellation library and lockfile churn.

## Dedicated `jj` revision boundary

Create one dedicated stacked revision, for example
`feat(protocol): cancel expired sidecar callbacks`, containing only:

```text
crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare
crates/sidecar-protocol/src/{protocol.rs,wire.rs}
crates/native-sidecar/src/{service.rs,stdio.rs}
crates/native-sidecar/tests/protocol.rs
crates/sidecar-client/src/transport.rs

packages/runtime-core/src/{generated-protocol.ts,frame-rpc.ts,protocol-frames.ts,protocol-client.ts,sidecar-errors.ts}
packages/runtime-core/tests/{frame-rpc.test.ts,protocol-frames.test.ts,protocol-client.test.ts,shared-sidecar-ownership.test.ts}

crates/agentos-protocol/protocol/agent_os_acp_v1.bare
crates/agentos-sidecar/src/acp_extension.rs
crates/agentos-sidecar/tests/acp_extension.rs
crates/client/src/{agent_os.rs,session.rs,transport.rs}

packages/core/src/agent-os.ts
packages/core/src/sidecar/agentos-protocol.ts
packages/core/tests/session-config-routing.test.ts
packages/core/tests/agent-os-dispose-retry.test.ts
packages/core/tests/permission-no-handler-warning.test.ts
packages/core/tests/cross-session-permission-reply.test.ts
packages/core/tests/permission-handler-failure.test.ts  # if Item 67 landed

docs/thin-client-migration.md
```

Generated Rust wire/ACP code remains build output. Additional compile-only
exhaustive-match edits in existing protocol tests are acceptable, but do not
include native-browser ACP policy, cron reliability, public host-callback API
redesign, or unrelated generated changes. No Cargo or pnpm lockfile change is
expected.

Focused validation:

```sh
pnpm --dir packages/build-tools build:protocol
pnpm --dir packages/core build:agentos-protocol
cargo test -p agentos-sidecar-protocol
cargo test -p agentos-native-sidecar cancellable_sidecar_callback
cargo test -p agentos-native-sidecar callback_response_and_cancellation
cargo test -p agentos-sidecar-client
cargo test -p agentos-sidecar permission_callback
cargo test -p agentos-client permission
pnpm --dir packages/runtime-core test -- protocol-frames.test.ts frame-rpc.test.ts protocol-client.test.ts shared-sidecar-ownership.test.ts
pnpm --dir packages/core test -- session-config-routing.test.ts permission-handler-failure.test.ts permission-no-handler-warning.test.ts cross-session-permission-reply.test.ts agent-os-dispose-retry.test.ts
cargo check --workspace
pnpm check-types
```

The final tracker evidence should record the old TS/Rust grace-timer tests, the
native no-cancellation-frame observation, both native response/cancellation
race orders, TS/Rust transport cancellation tests, zero client timers, retained
sidecar-default tests, the explicitly documented already-in-flight response
race, and the dedicated `jj` revision ID.
