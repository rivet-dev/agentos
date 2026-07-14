# Item 58 research: make Rust Execute transport routing unskippable

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 58 tracker status.

Originally inspected on **2026-07-14** at revision **`ea02cbc40b22`** and
revalidated on the same date at **`810ce567c333`**. The current tracker anchors
are `docs/thin-client-migration.md:104` (issue inventory), line 191 (pending
status), and line 283 (before/after/complete checklist).

## Recommendation

Replace the public
`SidecarTransport::request_wire_with_process_events(OwnershipScope,
RequestPayload)` method with a typed
`SidecarTransport::execute_wire(OwnershipScope, ExecuteRequest)` operation.
Keep the generated `RequestPayload::ExecuteRequest` wire variant, but make every
generic `request_wire*` entry point reject it before allocating an ID, encoding a
frame, registering a pending request, or enqueueing bytes.

The low-level transport must remain the sole owner of Execute's atomic process
route and cancellation tombstone. A caller should not be able to opt out of that
safety behavior by choosing a more generic method that happens to accept the same
wire union.

Use a typed `TransportError::InvalidRequest` for the rejected generic call and
map it to Rust AgentOS's existing `ClientError::InvalidArgument`. Do not change
the sidecar protocol or move cleanup into a product client. The sidecar already
owns process creation and signal handling; this item closes an unsafe transport
API path around the existing behavior.

Priority: **P2**. Confidence: **high**. All production Rust Execute callers and
all cancellation state are in-repository, the safe path already exists, and the
wire protocol ships in lockstep. The change is an API hardening/rename plus a
pre-enqueue guard, not a new process lifecycle.

## Cross-layer disposition

| Layer | Exact current code | Item 58 disposition |
|---|---|---|
| Rust low-level transport | `crates/sidecar-client/src/transport.rs:629-752`, response correlation at `:839-898`, cancellation cleanup at `:394-470` | **Change.** Add the typed Execute entry point, reject Execute in all generic entry points before side effects, and make Execute mode select the existing route/tombstone behavior. |
| Rust product client | `crates/client/src/process.rs:665-710` and `crates/client/src/shell.rs:127-181` | **Change.** Pass the already-built `wire::ExecuteRequest` directly to `execute_wire`; retain all response and event handling. |
| Rust error mapping | `crates/sidecar-client/src/error.rs:3-13` and `crates/client/src/error.rs:76-82` | **Change.** Add local `InvalidRequest` and map it to existing `InvalidArgument`. |
| BARE/generated protocol | `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:373-387,502-545,674-677,907`; compatibility tags and expected-response mapping in `crates/sidecar-protocol/src/protocol.rs:1600,1744-1791,1813,2562-2580` | **No change.** Execute must remain union tag 14 and must still expect `ProcessStarted`; only the Rust transport entry point changes. |
| Native sidecar/runtime | process start in `crates/native-sidecar/src/execution.rs:3753`, browser dispatch in `crates/native-sidecar-browser/src/wire_dispatch.rs:1907`, and their existing kill paths | **No change.** These already own process creation, the authoritative process ID, events, and signals. Moving the host-future cancellation race into them would require a new protocol lifecycle and duplicate transport correlation. |
| TypeScript runtime/client | typed process wrapper in `packages/runtime-core/src/sidecar-process.ts:1453-1517`, live request variant in `packages/runtime-core/src/request-payloads.ts:175-190` and conversion at `:471-490`, product call in `packages/core/src/sidecar/rpc-client.ts:777-804` | **No change.** TypeScript uses a different typed process wrapper; the bypass under review is the Rust transport's public union-accepting API. Keep the wire Execute variant intact. |
| Public docs/compatibility mirror | Item 58 tracker rows above; no public API docs mention the private Rust transport helper and the generated secure-exec mirror has no matching call | **Tracker evidence only.** Regenerate the mirror as a required check, but no content diff is expected. |

The important boundary is that this item does **not** move extra behavior into
the sidecar. The sidecar already implements the Linux process behavior. It makes
the Rust transport unable to send a process-start request without also enabling
the correlation needed to observe or kill that process if the host future is
cancelled.

## Exact transport surface and visibility

The following inventory was rechecked repository-wide at `810ce567c333`. These
are the only public/private signatures that select Execute's transport
lifecycle:

| Visibility | Current symbol and type | Current callers | Required result |
|---|---|---|---|
| `pub` | `SidecarTransport::request_wire(OwnershipScope, RequestPayload) -> Result<ResponsePayload, TransportError>` | Generic Rust product operations plus `CancelledProcessCleanup::drop` for `KillProcessRequest` | Retain, but reject `RequestPayload::ExecuteRequest` before any request ID or frame side effect. |
| `pub` | `SidecarTransport::request_wire_bounded(OwnershipScope, RequestPayload, usize) -> Result<ResponsePayload, TransportError>` | `crates/client/src/net.rs` for bounded fetch | Retain, with the same centralized Execute rejection. |
| `pub` | `SidecarTransport::request_wire_with_response_hook<F>(OwnershipScope, RequestPayload, F) -> Result<ResponsePayload, TransportError>` where `F: FnOnce(&ResponsePayload) -> Result<(), TransportError> + Send + Sync + 'static` | `crates/client/src/session.rs` for atomic ACP correlation | Retain, with the same Execute rejection; a response hook must not opt into process routing. |
| `pub` | `SidecarTransport::request_wire_with_process_events(OwnershipScope, RequestPayload) -> Result<(ResponsePayload, Option<WireEventSubscription>), TransportError>` | Exactly `AgentOs::send_execute` and `AgentOs::open_shell` | Replace with `execute_wire(OwnershipScope, ExecuteRequest)` returning the same tuple. |
| `pub` | `SidecarTransport::next_request_id(&self) -> RequestId` | No call outside `transport.rs`; repository search finds no consumer | Make private. ID allocation is transport state, not a client operation, and the after test can still inspect `request_counter` from the inline test module. |
| private | `request_wire_with_frame_limit(OwnershipScope, RequestPayload, Option<usize>, bool, Option<WireResponseHook>) -> Result<PendingWireResponse, TransportError>` | The four public operations above | Replace the boolean with private `WireRequestMode::{Generic, Execute}` and validate the mode/payload pair before calling the now-private `next_request_id`. |
| private | `PendingWireRequest { tx, process_events, response_hook }`, `PendingWireResponse { payload, process_events, cancel_cleanup, response_hook_error }`, and `PendingRequestGuard` | Registration, reader correlation, and cancellation cleanup inside `transport.rs` only | Preserve. Derive provisional subscription/tombstone retention from `WireRequestMode::Execute`; only arm started-process cleanup when the pending record has that subscription. |
| `pub` type | `WireEventSubscription` | Rust process and shell consumers | Preserve. This is host-only routed event state the sidecar cannot consume on behalf of the Rust caller. |

`wire::RequestPayload` must remain public and must retain its generated
`ExecuteRequest(ExecuteRequest)` variant because it is the BARE protocol union.
The smallest safe change is therefore a typed public Execute operation plus a
single private pre-encoding guard; duplicating the entire non-Execute protocol
union into another Rust enum would be larger, drift-prone, and unnecessary.

## Original issue and exact failure

The generated protocol must contain `ExecuteRequest`:

- `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:502-545` includes it
  in `RequestPayload`;
- `crates/sidecar-protocol/src/protocol.rs:1744-1791` gives it union tag 14; and
- `crates/sidecar-protocol/src/protocol.rs:2562-2580` declares
  `ProcessStarted` as its expected response.

That wire representation is correct. The defect is that the public Rust
transport currently exposes two ways to send the same variant with different
cancellation semantics.

### Unsafe generic path

`SidecarTransport::request_wire` at
`crates/sidecar-client/src/transport.rs:628-638` accepts any
`wire::RequestPayload`, including `ExecuteRequest`, and calls
`request_wire_with_frame_limit(..., subscribe_process_events = false, ...)`.
The bounded and response-hook variants at current lines 650-682 have the same
generic payload input and also pass `false`.

The shared helper at current lines 704-752 then:

1. allocates a request ID and encodes the frame;
2. registers a pending oneshot with no provisional process subscription;
3. creates `PendingRequestGuard { remove_on_drop: true }`;
4. enqueues the encoded request; and
5. disarms the guard only when `subscribe_process_events` is true or a response
   hook was supplied.

If a caller sends Execute through plain `request_wire` and its future is
cancelled after step 4, `PendingRequestGuard::drop` at current lines 1038-1067
removes the pending entry. The sidecar may already have committed process
creation. When `ProcessStartedResponse` arrives, the reader finds no entry at
current lines 839-898, logs “response for unknown request id,” and cannot learn
the process ID needed for cleanup. The process has neither an event route nor a
kill tombstone and can continue as an orphan from the caller's perspective.

### Existing safe path

`request_wire_with_process_events` at current lines 684-702 performs a runtime
variant check, then calls the same helper with `subscribe_process_events = true`.
That changes the lifecycle in three load-bearing ways:

- `WireEventLog::subscribe_provisional()` is stored in the pending request;
- after successful enqueue, the pending guard is disarmed so cancellation
  retains a bounded tombstone; and
- the reader binds the provisional subscription to exact
  `(full ownership, process_id)` before waking the waiter.

When a `ProcessStartedResponse` is decoded, the reader also arms
`CancelledProcessCleanup` at current lines 855-880. If the request waiter has
already been cancelled, sending the response through the oneshot fails, the
delivered value drops, and `CancelledProcessCleanup::drop` at current lines
424-470 asynchronously sends sidecar-owned `KillProcess(SIGKILL)` for the exact
process. If the waiter receives the response normally, the helper disarms that
cleanup at lines 742-751 and returns the bound event subscription.

The safe behavior is already covered in pieces, but the public safe method still
takes the entire request union and generic methods do not prohibit Execute. API
shape, rather than convention, should select this lifecycle.

## Current production call inventory

Repository-wide search finds exactly two product uses of the specialized path:

| Caller | Current location | Use |
|---|---|---|
| `AgentOs::send_execute` | `crates/client/src/process.rs:665-710` | All Rust `exec`, `exec_argv`, and `spawn` starts. |
| `AgentOs::open_shell` | `crates/client/src/shell.rs:127-181` | PTY-backed shell start. |

Both already use the safe method. No production Rust source sends
`RequestPayload::ExecuteRequest` through plain `request_wire`,
`request_wire_bounded`, or `request_wire_with_response_hook` today. That is why
the current product paths are correct and the issue remains an exposed low-level
API footgun.

Native sidecar code that constructs an `ExecuteRequest` internally for cron or
dispatch is not a client transport caller and is outside this item.

## Exact production edits

### `crates/sidecar-client/src/error.rs`

Add a typed transport misuse variant:

```rust
/// A request selected a transport operation that cannot provide its required routing semantics.
#[error("invalid transport request: {0}")]
InvalidRequest(String),
```

This error is produced before anything reaches the sidecar. Do not report it as
`TransportError::Sidecar`; that would falsely attribute a local API misuse to the
sidecar.

### `crates/client/src/error.rs`

Extend `impl From<TransportError> for ClientError` at current lines 68-75:

```rust
TransportError::InvalidRequest(message) => ClientError::InvalidArgument(message),
```

No normal AgentOS product call should hit this branch after its two Execute
callers move to the typed operation. The mapping keeps the conversion exhaustive
and preserves the correct local-invalid-input classification for any future
internal misuse.

### `crates/sidecar-client/src/transport.rs`

#### Hide request-ID allocation

Change `pub fn next_request_id` to private `fn next_request_id`. Repository-wide
search finds no external caller, and callers must never allocate transport IDs
independently of pending registration. The inline transport tests retain access
to both the method and `request_counter` through Rust module privacy.

#### Add a private request mode

Replace the ambiguous `subscribe_process_events: bool` parameter with a private,
two-case mode, for example:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WireRequestMode {
	Generic,
	Execute,
}
```

The three generic public operations pass `WireRequestMode::Generic`.
`execute_wire` passes `WireRequestMode::Execute`. Keep response-hook retention as
the separate existing concern; a generic response hook must not make Execute
legal.

#### Reject Execute centrally before encoding

At the very start of `request_wire_with_frame_limit`, before
`next_request_id()`, validate the payload/mode pair:

```rust
let is_execute = matches!(&payload, wire::RequestPayload::ExecuteRequest(_));
match (mode, is_execute) {
	(WireRequestMode::Generic, true) => {
		return Err(TransportError::InvalidRequest(String::from(
			"ExecuteRequest must use SidecarTransport::execute_wire",
		)));
	}
	(WireRequestMode::Execute, false) => {
		return Err(TransportError::InvalidRequest(String::from(
			"SidecarTransport::execute_wire requires ExecuteRequest",
		)));
	}
	_ => {}
}
```

The second arm is an internal invariant guard; the typed public method makes it
unreachable to normal callers. Keeping the check centralized means
`request_wire`, `request_wire_bounded`, and
`request_wire_with_response_hook` cannot diverge.

Derive provisional process-event subscription and post-enqueue tombstone
retention from `mode == WireRequestMode::Execute`, replacing the current boolean
checks. Preserve response-hook tombstones for non-Execute resource-establishing
extension requests.

Do not place the rejection after encoding or enqueue. The after test must prove
that invalid generic Execute neither consumes a request ID nor writes a frame.

#### Replace the public specialized method

Delete `request_wire_with_process_events`. Add:

```rust
/// Issue Execute while atomically binding its process event route and retaining
/// post-enqueue cancellation cleanup until ProcessStarted is decoded.
pub async fn execute_wire(
	&self,
	ownership: wire::OwnershipScope,
	request: wire::ExecuteRequest,
) -> Result<
	(wire::ResponsePayload, Option<WireEventSubscription>),
	TransportError,
> {
	let response = self
		.request_wire_with_frame_limit(
			ownership,
			wire::RequestPayload::ExecuteRequest(request),
			None,
			WireRequestMode::Execute,
			None,
		)
		.await?;
	Ok((response.payload, response.process_events))
}
```

Keeping `Option<WireEventSubscription>` preserves the current rejected-response
shape: a `RejectedResponse` has no bound process route, while a valid
`ProcessStartedResponse` must return one and the product client already checks
that invariant. Do not collapse sidecar `RejectedResponse { code, message }`
into a generic transport error; `crates/client` maps it to
`ClientError::Kernel` with the original code.

Add rustdoc stating that all generic request methods reject Execute. A small
`compile_fail` example should show that `execute_wire` accepts `ExecuteRequest`,
not `RequestPayload::ExecuteRequest(request)`. Do not claim that the protocol's
generic request union cannot represent Execute: it must. Its remaining generic
transport entry points exclude Execute with the tested pre-enqueue runtime
guard.

#### Tie cleanup to the Execute pending record

In the response arm at current lines 839-890, arm
`CancelledProcessCleanup` only when the pending request has a provisional
process subscription and the response is `ProcessStartedResponse`. After the
generic guard, a valid started response should already have that subscription;
making the relationship explicit prevents a mismatched generic response from
manufacturing Execute cleanup semantics.

Keep all existing bounded-log, exact-ownership route binding,
`CancelledProcessCleanup`, silence-watchdog, response-hook, writer-priority, and
pending-limit code.

### `crates/client/src/process.rs`

At `AgentOs::send_execute` (current lines 665-710), replace:

```rust
.request_wire_with_process_events(
	ownership,
	wire::RequestPayload::ExecuteRequest(build_process_execute_request(...)),
)
```

with:

```rust
.execute_wire(ownership, build_process_execute_request(...))
```

Keep `build_process_execute_request`, response/rejection mapping, and the
requirement that a successful start contains a bound event subscription. Do not
fold Item 59's post-start stdin/EOF failure handling into this revision.

### `crates/client/src/shell.rs`

At `AgentOs::open_shell` (current lines 127-181), keep building the same typed
`wire::ExecuteRequest`, then replace the union-wrapped specialized call with:

```rust
let (response, events) = self
	.transport()
	.execute_wire(ownership.clone(), execute)
	.await?;
```

Keep PTY options, response validation, event pumping, route-failure cleanup, and
shell registry behavior unchanged.

## Exact test work

All transport regression tests belong in the existing inline test module in
`crates/sidecar-client/src/transport.rs`. No new fake transport layer is needed.

### Before test: reproduce the bypass

Before adding the generic guard, add a temporary test named approximately
`generic_execute_cancelled_after_enqueue_loses_cleanup_route`:

1. construct an `Arc<SidecarTransport>` with a visible `request_writer_rx`, as
   the existing cancellation tests do;
2. spawn `request_wire(vm_ownership,
   RequestPayload::ExecuteRequest(test_execute_request()))`;
3. receive and decode the outbound Execute frame, proving enqueue completed;
4. abort the request task and assert the pending count becomes zero;
5. deliver a matching `ProcessStartedResponse` through `handle_wire_frame`; and
6. assert no `KillProcessRequest` appears on `request_writer_rx` within a short
   bounded timeout.

That test passes on the vulnerable parent and is the tracker checkbox's concrete
before evidence: cancellation removed the only correlation record after the
sidecar could have committed process creation. Record its command/result in the
tracker, then replace it with the after regression; do not commit a test that
endorses the orphaning behavior.

### After test: generic paths reject before enqueue

Add a committed test named approximately
`generic_request_paths_reject_execute_before_enqueue`. Exercise all three public
generic operations with fresh Execute payloads:

- `request_wire`;
- `request_wire_bounded`; and
- `request_wire_with_response_hook`.

For each, assert:

- the result is `TransportError::InvalidRequest` naming `execute_wire`;
- no outbound frame is available;
- `pending_request_count` remains zero;
- the response hook is not called; and
- the request counter is unchanged, proving rejection occurred before ID
  allocation/encoding.

The generated `RequestPayload` still being able to represent Execute is not a
failure—the protocol requires it. The contract is that no generic transport
operation can successfully encode or enqueue it.

### Exercise the real specialized API in cancellation tests

The current
`cancelled_execute_retains_tombstone_and_kills_started_process` test at lines
2056-2141 manually calls `register_pending` and constructs a disarmed guard. It
proves the pieces but not that the public method selects them. Rewrite its
post-enqueue case to:

1. spawn `execute_wire(ownership, test_execute_request())`;
2. receive/decode its Execute frame and capture the allocated request ID;
3. abort the task after enqueue and assert one pending tombstone remains;
4. deliver `ProcessStartedResponse` for that request ID;
5. assert an exact-owner `KillProcessRequest { signal: "SIGKILL" }` is emitted;
6. deliver `ProcessKilledResponse`; and
7. assert the pending map is empty.

Retain deterministic coverage for cancellation after a start response is
buffered but before the waiter consumes it. The current manual oneshot test is a
valid way to force that timing without a scheduler race; rename/split it if
needed rather than losing the case.

Update `execute_cancelled_before_enqueue_removes_pending_slot` at current lines
2143-2199 to call `execute_wire` with a typed request. Keep its full writer queue
and zero-pending assertion. Also retain
`process_route_is_bound_before_started_response_is_observed` and the response
hook cancellation test unchanged except for any helper renames.

### Compile/API coverage

The two production call sites compiling with `ExecuteRequest` provide positive
typed API coverage. Add a rustdoc `compile_fail` example beside `execute_wire`
that attempts to pass `RequestPayload::ExecuteRequest(request)` to the typed
method (or calls the removed `request_wire_with_process_events` API). Run doctests
explicitly. Combined with the pre-enqueue generic rejection test, this proves:

- the only process-routing operation accepts a typed `ExecuteRequest`;
- the old union-accepting specialized API is gone; and
- the remaining generic union-accepting APIs cannot transmit Execute, although
  the wire union correctly remains able to represent it at compile time.

Do not add `trybuild` solely for this item; rustdoc plus the runtime transport
test covers the public shape without a new dependency or lockfile churn.

## Before and after checklist

### Before behavior

- [ ] The temporary generic-path cancellation test enqueues Execute, aborts its
  waiter, observes the pending entry disappear, and receives no cleanup kill
  after `ProcessStartedResponse`.
- [ ] Repository search confirms production `crates/client` callers happen to
  use `request_wire_with_process_events`, showing safety currently depends on
  convention rather than the generic API.
- [ ] Baseline `cargo test -p agentos-sidecar-client --lib` passes the existing
  28 transport tests, including specialized before-enqueue, route-binding, and
  cancellation cleanup cases.

Baseline evidence recorded during this research at `ea02cbc40b22`:

| Command | Result |
|---|---|
| `cargo test -p agentos-sidecar-client --lib` | **pass: 28 passed, 0 failed** |
| `cargo test -p agentos-sidecar-client --test wire_protocol` | **pass: 3 passed, 0 failed** |
| `cargo test -p agentos-client --lib` | **pass: 69 passed, 0 failed** |

### After behavior

- [ ] All generic `request_wire*` variants return
  `TransportError::InvalidRequest` for Execute before ID allocation, pending
  registration, encoding, enqueue, or hook invocation.
- [ ] `execute_wire` accepts `wire::ExecuteRequest` directly; the old
  union-accepting specialized method is absent and its compile-fail doctest
  passes.
- [ ] Post-enqueue cancellation through the real `execute_wire` method retains
  one bounded tombstone, binds the authoritative process ID, and sends
  `SIGKILL` if the waiter is gone.
- [ ] Before-enqueue cancellation removes its pending slot, and buffered-start
  cancellation still triggers cleanup.
- [ ] `AgentOs::send_execute` and `AgentOs::open_shell` compile and retain their
  current response/event behavior.
- [ ] Rust process and shell E2Es remain green in the explicit expensive phase.
- [ ] The generated secure-exec compatibility mirror is regenerated; because it
  is a pure Rust re-export shim, no content change is expected.
- [ ] Item 58 is marked `done` only after evidence is recorded in the tracker.

Focused validation commands:

```sh
cargo fmt --all -- --check
cargo test -p agentos-sidecar-client --lib
cargo test -p agentos-sidecar-client --doc
cargo test -p agentos-sidecar-client --test wire_protocol
cargo test -p agentos-client --lib
cargo check -p agentos-client
cargo check --workspace
node scripts/generate-secure-exec-mirror.mjs
git diff --check
```

Run these real-sidecar suites in the explicit expensive phase with the required
sidecar/software assets built:

```sh
cargo test -p agentos-client --test process_e2e
cargo test -p agentos-client --test shell_e2e
```

## Client-to-sidecar test migration

None is appropriate for Item 58. The sidecar already owns process creation,
process IDs, events, and `KillProcess`; the defect is cancellation correlation
inside the Rust stdio transport after a host future is dropped. Only a transport
test can deterministically cancel between enqueue and response and inspect the
pending tombstone.

Keep native sidecar Execute/signal tests where they are. They prove the sidecar
starts and kills processes correctly, but moving this regression there would not
exercise the unsafe choice between Rust transport entry points. No TypeScript
test changes are needed because the exposed bypass is Rust-specific.

## Dependencies and overlap

- **Item 22 is the required parent.** It introduced exact-owner process event
  routes, the retained cancellation tombstone, `CancelledProcessCleanup`, and
  the three cancellation timing cases. Item 58 makes that completed safety path
  mandatory; it must not reimplement or simplify it away.
- **Item 59 should stack after Item 58.** Item 59 handles failures after a
  successful start while Rust `exec_request` writes stdin and EOF. Item 58 is
  only about cancellation of the start request itself. If Item 59 introduces an
  atomic finite-input operation, `spawn` and PTY shell starts still require this
  typed Execute path.
- **Item 57 is adjacent but independent.** Its result-bearing process-exit
  callback may edit `crates/client/src/process.rs`; preserve its callback/error
  changes while changing only `send_execute` here.
- **Item 46 may change presence-sensitive Execute fields.** Pass the typed
  `ExecuteRequest` through unchanged; do not normalize any `Option` values in
  this item.
- No protocol compatibility layer is required. Client, transport, and sidecar
  release in lockstep, and the wire union itself does not change.

## Risks and review points

- **Reject before enqueue.** A guard after frame send merely reports misuse
  after the sidecar may have started a process and does not close the orphan
  window.
- **Cover every generic variant.** `request_wire_bounded` and
  `request_wire_with_response_hook` are bypasses too, even though current
  product callers use them only for fetch and ACP extension requests.
- **Do not remove Execute from the protocol union.** The dedicated method must
  still encode that exact generated variant.
- **Do not make process IDs client-generated.** Atomic binding depends on the
  authoritative ID in `ProcessStartedResponse`.
- **Do not drop rejected-response fidelity.** The product client must retain the
  sidecar's `{ code, message }` and bound routes only for successful starts.
- **Do not weaken post-enqueue tombstones.** They remain bounded by
  `PENDING_REQUEST_LIMIT` and terminated by response, transport failure, or the
  silence watchdog.
- **Do not conflate request cancellation with process timeout.** Execute's
  `timeout_ms` is enforced by the sidecar after start; this transport cleanup is
  for a host waiter that disappears during the start handshake.
- **Do not add a second client cleanup state machine.** `crates/client` should
  call the typed operation and consume its result; the low-level transport owns
  response ordering and cancellation races.
- **Keep Item 59 separate.** Stdin write/close failures happen after
  `execute_wire` returns successfully and need their own fail-closed behavior.

## Bounded dedicated JJ revision

Apply the revision in this order so every intermediate compiler failure points
at the next required edit:

1. Add `TransportError::InvalidRequest` and its `ClientError::InvalidArgument`
   conversion.
2. Add `WireRequestMode`, the pre-ID mode/payload guard, mode-derived
   subscription/tombstone behavior, and the subscription-gated cleanup arm;
   make `next_request_id` private.
3. Add typed `execute_wire`, then migrate `send_execute` and `open_shell` before
   deleting `request_wire_with_process_events`.
4. Add the generic-path rejection test and rewrite the two Execute cancellation
   tests to enter through `execute_wire`; retain the deterministic buffered
   response timing case.
5. Add/run the compile-fail doctest, focused crate tests, workspace checks, and
   compatibility mirror generator.
6. Record exact before/after commands in `docs/thin-client-migration.md`, mark
   Item 58 done, and keep that tracker update in this same dedicated revision.

Create one new stacked JJ revision for Item 58 and keep it to:

```text
crates/sidecar-client/src/error.rs
crates/sidecar-client/src/transport.rs
crates/client/src/error.rs
crates/client/src/process.rs
crates/client/src/shell.rs
docs/thin-client-migration.md  # evidence/status only, last
```

No BARE schema, generated protocol, native/browser sidecar, TypeScript client,
Cargo manifest, lockfile, or website edit is expected. Run the secure-exec
mirror generator as required by the project boundary; its pure re-export shim
should remain unchanged and should not broaden the AgentOS revision path set.
