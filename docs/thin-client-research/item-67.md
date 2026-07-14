# Item 67 research: fail closed when a TypeScript permission handler throws

Status: implementation-ready research only. This note does not modify
production code, tests, or the Item 67 tracker status.

Inspected on **2026-07-14** at revision **`e6e930e5`**. Tracker anchors are
`docs/thin-client-migration.md:113` (issue inventory), current line 194
(pending status), and current line 280 (before/after/complete checklist).

## Recommendation

Move permission-handler invocation out of the `Promise` constructor in
`AgentOs._handleAcpPermissionCallback`. If a synchronous handler throws, remove
the exact pending route immediately, clear its cleanup timer, log the original
failure, stop invoking later handlers, and return no host reply. The enclosing
ACP callback must still encode a valid `AcpPermissionCallbackResponse` with
`reply: null`, leaving the authoritative default to the sidecar.

Priority: **P1**. Confidence: **high**.

Use an explicit **fail-fast** delivery contract for the handler set: handlers
run in registration order until one throws. A thrown handler invalidates that
host dispatch, later handlers are not called, and even a reply synchronously
attempted before the throw is discarded for this sidecar callback. Continuing
fanout after deleting the route would invite later handlers to answer a route
that is already invalid and would turn their normal `respondPermission` calls
into secondary rejected promises.

The ownership boundary remains:

```text
sidecar
  owns adapter request, timeout, default reply, and ACP result translation
     |
     | typed callback + post-decision cleanup deadline
     v
TypeScript client
  owns only host-handler delivery and temporary reply correlation
     |
     | explicit host reply, or null after route/handler failure
     v
sidecar applies the result/default
```

Do not move JavaScript handler execution into the sidecar; the closure and host
application state are inaccessible there. Do not make TypeScript select
`"reject"` on failure; `null` is the transport statement that no host answer was
produced.

## Original issue

The tracker entries are at `docs/thin-client-migration.md:113,194,280`:

> A synchronous TypeScript permission-handler exception rejects the callback
> but leaves its pending reply entry and timer alive until delayed cleanup, and
> later handlers are not invoked.

The current outer catch correctly returns `undefined`, so the sidecar eventually
owns the permission result. The resource/correlation cleanup is wrong:

- the pending entry remains addressable through `respondPermission` after the
  callback has already returned no answer;
- its cleanup timer stays live until `cleanupAfterMs`, currently the sidecar
  deadline plus grace;
- `respondPermission` can report a successful local reply against that stale
  entry even though the reply can no longer affect the completed sidecar
  callback;
- the first thrown handler aborts iteration accidentally through JavaScript's
  `Promise` constructor semantics rather than an explicit delivery contract;
  and
- if a handler resolves the stored reply and then throws, the earlier resolve
  wins, the constructor's implicit reject is ignored, the failure is not logged,
  and TypeScript can return the host-selected reply.

The last case is why adding only `delete`/`clearTimeout` to the existing outer
catch is insufficient. Host code must not execute inside the promise executor.

## Exact current code and failure mechanics

### Pending route shape

`AgentSessionEntry` in `packages/core/src/agent-os.ts:208-228` owns:

```ts
pendingPermissionReplies: Map<
	string,
	{
		resolve: (reply: PermissionReply) => void;
		reject: (error: Error) => void;
		cleanupTimer: ReturnType<typeof setTimeout>;
	}
>;
```

This is legitimate host-only correlation. The sidecar cannot call a JavaScript
closure directly, while `respondPermission(sessionId, permissionId, reply)`
must find the exact callback wait associated with the session and permission.
`sessionEntryFromRoute` initializes the map at current lines 774-789.

### Handler invocation occurs inside a `Promise` executor

`_handleAcpPermissionCallback` at current
`packages/core/src/agent-os.ts:2908-2961`:

1. returns `undefined` for an unknown session;
2. returns `undefined` and emits the no-handler warning when the handler set is
   empty;
3. constructs a `Promise<PermissionReply>` at lines 2927-2953;
4. installs the cleanup timer and pending map entry at lines 2928-2940; and
5. invokes every handler at lines 2950-2952 **inside that executor**.

JavaScript catches a synchronous exception thrown by a promise executor and
turns it into a rejection of that promise. It does not unwind through the code
that installed the pending map entry. The outer `await` catches the rejection at
lines 2954-2960, logs it, and returns `undefined`, but neither deletes the entry
nor clears the timer.

Iteration also stops at the throwing handler. That behavior is currently an
incidental consequence of the throw. Item 67 should retain it deliberately as
fail-fast behavior and cover it with a named assertion.

There is a second promise-settlement edge:

```ts
vm.onPermissionRequest(sessionId, (request) => {
	void vm.respondPermission(sessionId, request.permissionId, "always");
	throw new Error("handler failed after replying");
});
```

`respondPermission` runs synchronously up to its returned resolved promise. It
deletes the pending entry, clears the timer, and calls the stored `resolve`.
When the handler then throws, the `Promise` constructor calls `reject`, but the
promise has already resolved. The outer catch never runs and the callback
returns `"always"`. Moving handler execution outside the executor makes the
throw authoritative for host-route validity and causes the sidecar callback to
receive `null` instead.

### Stale reply API behavior

`respondPermission` at `agent-os.ts:2997-3020` deletes and clears a route only
when the host explicitly replies. Because the thrown-handler path leaves the
entry behind, a later caller can receive a successful local JSON-RPC-shaped
result with `via: "sidecar-request"` even though the actual sidecar request has
already completed with no host reply. Once Item 67 removes the route, the same
call correctly throws:

```text
Permission request is not pending: <permissionId>
```

Session close/dispose cleanup at `agent-os.ts:2645-2666` already clears timers
and rejects all still-valid pending routes. Do not change that lifecycle
behavior in Item 67.

## The sidecar already owns the failure outcome

No protocol or Rust production change is required.

`_handleAcpExtSidecarRequest` at `agent-os.ts:2850-2905` decodes the generated
`AcpPermissionCallback`, awaits `_handleAcpPermissionCallback`, and encodes:

```ts
{
	tag: "AcpPermissionCallbackResponse",
	val: {
		permissionId: callback.val.permissionId,
		reply: reply ?? null,
	},
}
```

The generated field comment in
`packages/core/src/sidecar/agentos-protocol.ts:1321-1327` explicitly says the
client supplies only an explicit host answer and the sidecar owns the default
when the route is absent, times out, or fails.

On the native sidecar:

- `build_inbound_response` in
  `crates/agentos-sidecar/src/acp_extension.rs:1986-2054` creates the typed
  callback, owns `PERMISSION_CALLBACK_TIMEOUT`, and waits for the host route;
- `permission_callback_reply` at `acp_extension.rs:2874-2881` maps a missing
  reply to `"reject"`;
- `permission_callback_reply_from_result` at lines 2883-2898 applies the same
  sidecar default on the authoritative callback timeout; and
- the existing unit test
  `missing_client_permission_reply_uses_sidecar_default` at lines 3151-3171
  proves `reply: None` becomes the sidecar default while an explicit `once`
  remains `once`.

The TypeScript callback must return a valid typed response with `reply: null`
rather than throw out through the protocol handler. A thrown `ext` handler is
converted by `packages/runtime-core/src/callbacks.ts:69-95` to an `ext_result`
whose payload is raw error text; Rust cannot decode that text as
`AcpCallbackResponse`, so it becomes a transport/protocol error rather than the
sidecar-owned permission default.

## Exact production edit

Change only `_handleAcpPermissionCallback` in
`packages/core/src/agent-os.ts`.

### 1. Restrict the `Promise` executor to route setup

Create the reply promise, timer, and map entry first, with no user callback in
the executor:

```ts
const replyPromise = new Promise<PermissionReply>((resolve, reject) => {
	const cleanupTimer = setTimeout(() => {
		session.pendingPermissionReplies.delete(permissionId);
		reject(
			new Error(
				`Permission reply route expired after the sidecar deadline: ${permissionId}`,
			),
		);
	}, cleanupAfterMs);
	session.pendingPermissionReplies.set(permissionId, {
		resolve,
		reject,
		cleanupTimer,
	});
});
```

Keep the current sidecar-provided `cleanupAfterMs`; Item 68 owns removal of the
post-decision grace timer after the protocol gains explicit cancellation or
expiry acknowledgement.

### 2. Dispatch handlers in an explicit fail-fast block

Build the existing raw `PermissionRequest`, then invoke a snapshot in
registration order:

```ts
try {
	for (const handler of [...session.permissionHandlers]) {
		handler(permissionRequest);
	}
} catch (error) {
	const pendingReply = session.pendingPermissionReplies.get(permissionId);
	if (pendingReply) {
		session.pendingPermissionReplies.delete(permissionId);
		clearTimeout(pendingReply.cleanupTimer);
	}
	console.warn(
		`ACP permission handler failed for ${sessionId}/${permissionId}; the host route was removed and the sidecar owns the outcome`,
		error,
	);
	return undefined;
}
```

Snapshotting the set prevents handler registration/removal during delivery from
changing which already-registered handlers belong to this dispatch. Stop on the
first failure. Do not call `pendingReply.reject(error)`: this function is no
longer awaiting `replyPromise` on the failure path, so rejecting it would create
an unhandled rejection. Clearing the sole timer and dropping the map entry makes
the now-unreachable pending promise collectable.

If an earlier handler already called `respondPermission`, the map entry and
timer are already gone. Still log the later failure and return `undefined`;
ignore the resolved `replyPromise`. This is how the client avoids selecting the
permission result after incomplete host fanout.

### 3. Await the reply only after successful delivery

Retain the current route-expiry/session-close warning behavior around the wait:

```ts
try {
	return await replyPromise;
} catch (error) {
	console.warn(
		`ACP permission callback route closed for ${sessionId}/${permissionId}; the sidecar owns the outcome`,
		error,
	);
	return undefined;
}
```

Do not add an implicit `"reject"`, `"once"`, or `"always"` result in either
catch. Do not add a client retry or re-run failed handlers.

### Async handler returns are outside this bounded item

The public core `PermissionRequestHandler` currently returns `void`. Item 67 is
specifically the synchronous exception path. Do not silently change it to an
awaited multi-handler API in this revision: that changes callback ordering and
how much of the sidecar deadline a handler may consume. A separately tracked
API decision can widen it to `void | Promise<void>` with explicit rejection and
deadline tests if core users need asynchronous handlers. The actor-level
`onPermissionRequest` hook in `packages/agentos` is a different API and already
declares `void | Promise<void>`.

## Before and after tests

Add a focused `packages/core/tests/permission-handler-failure.test.ts`. Use
`Object.create(AgentOs.prototype)` plus a narrow `_sessions` injection, matching
`session-config-routing.test.ts` and `permission-no-handler-warning.test.ts`.
This avoids starting a sidecar merely to prove host correlation cleanup.

Import `encodeAcpCallback` and `decodeAcpCallbackResponse` from the generated
AgentOS protocol module and drive `_handleAcpExtSidecarRequest` with a real
`dev.rivet.agent-os.acp` envelope. Testing the full private envelope method,
rather than only `_handleAcpPermissionCallback`, proves the reply sent back to
the sidecar is structurally valid and null.

### Before-behavior evidence

With fake timers, register two handlers. The first throws a retained `Error`
object synchronously and the second is a spy. On the vulnerable parent:

1. the returned callback response decodes with `reply: null`;
2. the first handler ran and the second did not;
3. `pendingPermissionReplies` still contains the permission ID immediately
   after the callback returned;
4. `vi.getTimerCount()` reports the live cleanup timer; and
5. the warning contains the exact thrown error.

Advance to `cleanupAfterMs` and prove only then does the entry disappear. Record
this passing vulnerable-parent command/revision in the tracker, then replace
the stale-route expectations with the after assertions.

### After: thrown handler removes the route immediately

The lasting regression should assert, before advancing fake time:

- the encoded response is `AcpPermissionCallbackResponse` for the exact
  permission ID with `reply: null`;
- the first handler ran once and the second did not (documented fail-fast
  delivery);
- the pending map no longer contains the route and the cleanup timer count is
  zero;
- `respondPermission` rejects with `Permission request is not pending`;
- `console.warn` includes the session ID, permission ID, sidecar-owns-outcome
  text, and the exact original `Error` object; and
- advancing beyond `cleanupAfterMs` produces no delayed cleanup side effect or
  second warning.

### After: a reply followed by a throw is not accepted

Add a second case whose handler calls
`void agent.respondPermission(..., "always")` and then throws synchronously.
Assert the envelope still decodes to `reply: null`, the handler failure is
logged, and the route/timer are absent. On the vulnerable parent this returns
`reply: "always"` and does not reach the outer warning, so this regression proves
handler dispatch truly moved outside the promise executor.

### Retain adjacent coverage

- `packages/core/tests/session-config-routing.test.ts` proves the
  sidecar-provided post-decision cleanup deadline removes unanswered routes.
- `packages/core/tests/permission-no-handler-warning.test.ts` proves an absent
  handler returns no explicit answer and warns once.
- `packages/core/tests/cross-session-permission-reply.test.ts` proves equal
  permission IDs in different sessions cannot resolve each other.
- `crates/agentos-sidecar/src/acp_extension.rs` already has unit coverage that a
  null reply uses the sidecar default; no Rust test change is necessary.

## Validation commands

```bash
pnpm --dir packages/core exec vitest run \
  tests/permission-handler-failure.test.ts \
  tests/session-config-routing.test.ts \
  tests/permission-no-handler-warning.test.ts \
  tests/cross-session-permission-reply.test.ts \
  --fileParallelism=false
pnpm --dir packages/core check-types
cargo test -p agentos-sidecar missing_client_permission_reply_uses_sidecar_default
pnpm check-types
git diff --check
```

The Rust command is a retained boundary gate, not evidence of a Rust production
change.

## Dependencies, overlaps, and risks

- **Item 52 should precede Item 67.** It removes false permission delivery from
  session notifications while retaining this typed callback route. After Item
  52, line numbers move, but `_handleAcpPermissionCallback`,
  `onPermissionRequest`, `respondPermission`, and the pending map remain.
- **Item 54 is separate listener error work.** Do not broaden Item 67 into all
  session/event callbacks. Permission delivery blocks an adapter request and
  has unique reply correlation; event subscribers are best-effort observers.
- **Item 62 is permission-policy test cleanup.** Item 67 must not add binding or
  other permission evaluation to the host route. Authorization happens before
  the sidecar emits this callback.
- **Item 68 owns exact end-of-wait signalling.** Keep the sidecar-supplied
  cleanup deadline for normal unanswered routes until cancellation/expiry is
  explicit in the protocol. Item 67 only clears it early on a known local
  handler failure.
- **Fail-fast must be explicit.** Continuing after deleting the route makes
  remaining handlers observe an unanswerable request. Re-running handlers could
  duplicate host side effects. Test and comment the stop-on-first-failure rule.
- **Discard prior resolution after failure.** Handler dispatch must finish
  successfully before `replyPromise` is awaited. Otherwise a resolve-before-
  throw sequence silently converts incomplete fanout into a client-selected
  permission decision.
- **Preserve exact error identity in logs.** Do not rebuild the handler error
  from its message; pass it as the second `console.warn` argument.
- **No Rust client parity edit is needed.** Rust permission delivery uses a
  broadcast stream and `PermissionResponder`; a consumer error cannot unwind
  through the producer's route insertion. Its cleanup behavior is separately
  covered in `crates/client/src/session.rs`.

## Bounded JJ revision

Create one dedicated stacked Item 67 revision containing only:

```text
packages/core/src/agent-os.ts
packages/core/tests/permission-handler-failure.test.ts
docs/thin-client-migration.md
```

No protocol schema/generated file, runtime-core, Rust, actor, website, lockfile,
or package manifest edit is required. Preserve other agents' changes in
`agent-os.ts` and inspect the revision paths before describing/squashing it.

Recommended revision description:

```text
fix(core): clean up failed permission handlers
```

## Completion checklist

- [ ] Vulnerable-parent evidence proves a synchronous throw returns null but
  retains the pending entry/timer until delayed cleanup and skips later handlers.
- [ ] Handler invocation no longer occurs inside a `Promise` executor.
- [ ] The failure route is removed and its timer cleared before the callback
  returns.
- [ ] The full envelope response contains `reply: null`; no TypeScript default
  is selected.
- [ ] A resolve-before-throw handler cannot select the sidecar permission result.
- [ ] Fail-fast ordering and exact host-visible error reporting are tested.
- [ ] Adjacent no-handler, timeout-cleanup, cross-session, and sidecar-default
  tests pass.
- [ ] The dedicated Item 67 `jj` revision contains only the bounded paths above.
- [ ] `docs/thin-client-migration.md` records before evidence, after evidence,
  revision ID, and marks Item 67 `done` only after every check passes.
