# Item 52 research: remove legacy ACP permission notification handling

Status: **implementation-ready research only**. Revalidated against working-copy
change `sqnqyqws` (`36a6deec`) on 2026-07-14 while Item 81 was in progress. This
note does not change production code, tests, or Item 52's tracker status.

Priority: **P2**. Implementation confidence: **high** (raise the tracker's
current medium rating when Item 52 is implemented). The client branch is
demonstrably unanswerable, the shared classifier proves it has no valid current
producer, Rust already has the target thin-client behavior, and the native
sidecar already owns the working replacement. The remaining risk is merge
coordination while Items 44, 53, and 54 touch adjacent ACP routing, rather than
uncertainty about the behavioral fix.

## Recommended fix

Delete TypeScript's interpretation of permission-shaped ACP session
notifications and stop adding the synthetic `_acpMethod` property to typed
callback params. Keep only the generated `AcpPermissionCallback` route that
forwards raw adapter params to the host handler and returns the handler's reply
to the sidecar.

Do **not** add another sidecar state machine. The native sidecar already:

1. recognizes the adapter's `session/request_permission` JSON-RPC request;
2. sends a typed `AcpPermissionCallback` to the client;
3. owns the decision timeout and missing-answer default; and
4. maps the abstract host reply to an option ID the active adapter offered.

The implementation is therefore a client deletion plus conformance coverage,
not a production sidecar migration.

## Original legacy issue

TypeScript hard-codes both a pre-ACP method alias and the current ACP method at
`packages/core/src/agent-os.ts:628-629`:

```ts
const LEGACY_PERMISSION_METHOD = "request/permission";
const ACP_PERMISSION_METHOD = "session/request_permission";
```

`AgentOs._recordSessionNotification` at current lines 2183-2214 treats either
method as a host permission request:

```ts
if (
	notification.method === LEGACY_PERMISSION_METHOD ||
	notification.method === ACP_PERMISSION_METHOD
) {
	const params = toRecord(notification.params);
	const permissionId = params.permissionId;
	if (
		typeof permissionId === "string" ||
		typeof permissionId === "number"
	) {
		const request: PermissionRequest = {
			permissionId: String(permissionId),
			description:
				typeof params.description === "string"
					? params.description
					: undefined,
			params,
		};
		for (const handler of session.permissionHandlers) {
			handler(request);
		}
	}
}
```

That branch invokes the host handler without inserting the permission ID into
`session.pendingPermissionReplies`. `AgentOs.respondPermission` at current
lines 2968-2991 accepts only a pending typed callback and otherwise rejects:

```text
Permission request is not pending: <permission-id>
```

The result is a false API promise: the client tells the host that a permission
request exists, but the host cannot answer it. A normal handler that calls
`void vm.respondPermission(...)` can produce an unhandled rejected promise. If
an adapter ever duplicated the request as a compatibility notification, the
same host handler could also run twice.

Item 28 removed the old client-originated `request/permission` response
fallback. This leftover listener is the dead half of that former state machine.

## Exact callers and current producers

| File / symbol | What reaches it today | Item 52 disposition |
| --- | --- | --- |
| `packages/core/src/agent-os.ts:2432`, `AgentOs._handleSidecarEvent` | Calls `_recordSessionNotification` for the legacy structured event name `acp.session_event`. Item 53 separately removes this unproduced outer compatibility shape. | Do not widen Item 52 into the outer event-route cleanup; deleting the permission-method branch makes this caller unable to fabricate a permission callback in the meantime. |
| `packages/core/src/agent-os.ts:2494`, `AgentOs._handleAcpExtEvent` | Decodes the generated `AcpSessionEvent` callback and calls `_recordSessionNotification`. These events contain adapter **notifications**, principally `session/update`. | Keep the caller and session-update delivery. It must not reinterpret a notification as a request. |
| `crates/agentos-sidecar-core/src/engine.rs:806`, `AcpCore::encode_session_notification` | The sole current `AcpSessionEvent` producer serializes values previously classified as `Notification`, plus sidecar-generated `session/update` state notifications. | Keep unchanged. It cannot produce a valid permission request because requests are consumed before event encoding. |
| `crates/agentos-sidecar-core/src/engine.rs:4525`, `answer_inbound_request` | Every resumable engine path sends `id + method` frames directly to `AcpHost::handle_inbound_request`; blocking JSON-RPC does the same in `crates/agentos-sidecar-core/src/json_rpc.rs:93-110`. | Keep unchanged. This is the authoritative request/response boundary. |
| `crates/agentos-sidecar/src/acp_extension.rs:1970`, `build_inbound_response` | Native `session/request_permission` requests become a typed host callback, then an adapter JSON-RPC response with the original ID. | Keep unchanged except for the conformance assertions described below. Adapter method compatibility belongs here. |
| `crates/agentos-protocol/protocol/agent_os_acp_v1.bare:270-305` | Defines `AcpPermissionCallback` and its response, including the sidecar-supplied cleanup deadline. Generated TypeScript and Rust codecs carry this data without method recognition. | Keep unchanged. No protocol change is needed. |
| `crates/client/src/agent_os.rs:1279-1330`, `handle_acp_ext_callback` | Rust consumes only the typed callback and never examines ACP permission method strings. | Keep unchanged; it is the parity target for TypeScript. |

Repository-wide production search (excluding generated codecs and the Item 81
test harness) finds exactly two method producers/consumers: the native sidecar's
real `session/request_permission` arm and its adapter fixture. The only remaining
client-side method strings and `_acpMethod` synthesis are the three TypeScript
lines Item 52 deletes.

## Why permission method support belongs to the adapter sidecar

### Requests and notifications are different protocol operations

`classify_json_rpc_message` in
`crates/agentos-sidecar-core/src/behavior.rs:24-46` deliberately classifies a
message containing both `id` and `method` as `InboundRequest`, never as a
notification. A valid ACP permission request has both:

```json
{
  "jsonrpc": "2.0",
  "id": 99,
  "method": "session/request_permission",
  "params": { "permissionId": "perm-1", "options": [] }
}
```

All shared-core execution paths answer that class through
`answer_inbound_request` rather than retaining it as an `AcpSessionEvent`:

- async create: `crates/agentos-sidecar-core/src/engine.rs:1669-1675`;
- async initialize/resume: lines 1815-1821;
- restart: lines 2184-2190;
- async session request: lines 2324-2330;
- blocking JSON-RPC: `crates/agentos-sidecar-core/src/json_rpc.rs:93-110`; and
- host response write: `engine.rs:4525-4531`.

Only the distinct notification class is encoded as an `AcpSessionEvent` by
`AcpCore::encode_session_notification` at `engine.rs:806-818`. Therefore the
TypeScript permission-notification branch has no current production producer.

### Native sidecar already owns modern permission behavior

The native host adapter's exact path is:

1. `NativeCoreHost::handle_inbound_request` in
   `crates/agentos-sidecar/src/acp_extension.rs:194-203` sends the complete
   request to the native async broker.
2. `build_inbound_response` at lines 1970-2072 matches
   `session/request_permission` at lines 1983-2055 before filesystem, terminal,
   and generic host-extension methods.
3. It serializes the adapter's raw `params` into `AcpPermissionCallback`, sets a
   125-second client cleanup deadline, invokes the callback under the
   sidecar-owned 120-second decision deadline, and writes the response using the
   original adapter request ID.
4. `permission_callback_reply_from_result` at lines 2883-2897 applies the
   sidecar default on timeout; other callback failures propagate.
5. `permission_result` and `resolve_permission_option_id` at lines 2861-2925 map
   `once`, `always`, or `reject` to an option ID actually offered in this
   request. That preserves adapters such as OpenCode whose option ID is `once`
   rather than `allow_once`.

This is adapter-specific ownership in practice: the native adapter supports the
callback because it has a client callback bridge. The browser adapter has no
such bridge and uses the default `AcpHost::handle_inbound_request` response from
`crates/agentos-sidecar-core/src/host.rs:81-90`; its own comment at
`crates/agentos-sidecar-browser/src/acp_host.rs:195-198` explicitly says it does
not advertise agent-to-client callback tooling.

If a named agent adapter later needs a nonstandard permission method, add a
tested compatibility branch in the native sidecar's inbound-request handling.
If the alias is adapter-specific, first retain enough sidecar session metadata
to gate it to that adapter; the current `NativeCoreProcess` records ownership
and session ID but not adapter identity, so adding an unconditional alias to
`build_inbound_response` would incorrectly advertise it to every adapter. Do
not copy method strings into TypeScript or Rust.

### Rust already has the desired thin-client boundary

Rust's `handle_acp_ext_callback` at
`crates/client/src/agent_os.rs:1279-1330` decodes only the generated
`AcpPermissionCallback`, deserializes its raw params, routes the host closure by
VM/session ownership, and returns `AcpPermissionCallbackResponse`. Its session
event path does not interpret permission method strings.

Item 52 makes TypeScript match Rust. No Rust production edit is needed.

## Exact production edits

| File / symbol | Current issue | Exact edit |
| --- | --- | --- |
| `packages/core/src/agent-os.ts:628-629` | Client owns two adapter method constants. | Delete `LEGACY_PERMISSION_METHOD` and `ACP_PERMISSION_METHOD`. |
| `AgentOs._recordSessionNotification`, lines 2183-2214 | Permission-shaped notifications invoke a handler without an answer route. | Delete the entire second `if` block. Retain only `shouldDispatchToSessionEventHandlers` and `_dispatchSessionEvent`. |
| `AgentOs._handleAcpExtSidecarRequest`, lines 2836-2850 | Typed callback params are cloned and supplemented with `_acpMethod`. | Pass `toRecord(JSON.parse(callback.val.params))` directly to `_handleAcpPermissionCallback`. |

The resulting notification method should be exactly:

```ts
private _recordSessionNotification(
	session: AgentSessionEntry,
	notification: JsonRpcNotification,
): void {
	if (shouldDispatchToSessionEventHandlers(notification)) {
		this._dispatchSessionEvent(session, notification);
	}
}
```

The callback argument should change from:

```ts
{
	...toRecord(JSON.parse(callback.val.params)),
	_acpMethod: ACP_PERMISSION_METHOD,
}
```

to:

```ts
toRecord(JSON.parse(callback.val.params))
```

Keep these TypeScript pieces unchanged: `PermissionRequest`, `PermissionReply`,
`onPermissionRequest`, `respondPermission`, `permissionHandlers`,
`pendingPermissionReplies`, `_handleAcpPermissionCallback`, the cleanup range
check, the no-handler warning, and typed response encoding. They are required
host-side callback routing, not runtime policy.

No production changes belong in:

- `crates/agentos-sidecar/src/acp_extension.rs`;
- `crates/client/src/agent_os.rs`;
- `crates/agentos-sidecar-browser`;
- protocol schemas or generated protocol files; or
- `crates/native-sidecar/tests/acp_legacy`, which is a test-only record of the
  obsolete compatibility state machine. Do not partially rewrite it in Item
  52; replace and delete that harness as one separately tracked cleanup.

### Item 81 owns the adjacent legacy test harness

`crates/native-sidecar/tests/acp_legacy/{compat.rs,client.rs}` still contains a
complete duplicate of the old client permission normalization: both method
constants, `PendingPermissionRequests`, `_acpMethod` injection, synthetic
`request/permission` notifications, response option normalization, and its own
bounded pending-reply state. It is pulled into `acp_integration.rs`,
`acp_session.rs`, and `service.rs` only as test scaffolding; it is not linked
into a published client or sidecar.

That code is the separately tracked **Item 81**, now the active stacked change
at the time of this revalidation. Item 81 maps any narrow useful assertions to
the production shared/native ACP suites and deletes the whole harness. Item 52
must neither edit those files nor resurrect them if they are gone by the time
implementation starts. Deleting only the harness's permission branch would
leave a half-migrated fake client; moving any of its state into production would
recreate the architecture this work is removing.

## Exact test edits

### 1. Before characterization and lasting TypeScript regression

Add a table-driven test in
`packages/core/tests/session-config-routing.test.ts` covering both
`request/permission` and `session/request_permission`.

Before deleting production code, its assertions must be:

```ts
expect(permissionHandler).toHaveBeenCalledOnce();
expect(session.pendingPermissionReplies.size).toBe(0);
await expect(
	agent.respondPermission("session-1", "legacy-permission", "once"),
).rejects.toThrow("Permission request is not pending: legacy-permission");
```

This is the exact before proof: the legacy branch raises a permission callback
that cannot be answered. Run both table cases and record the passing command in
the tracker before changing `agent-os.ts`.

After deleting the branch, retain the test and change only the first assertion:

```ts
expect(permissionHandler).not.toHaveBeenCalled();
```

The final test can use this exact private-backdoor shape (before the production
deletion, change only `not.toHaveBeenCalled()` to `toHaveBeenCalledOnce()`):

```ts
it.each(["request/permission", "session/request_permission"])(
	"does not interpret the %s session notification as a permission callback",
	async (method) => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const permissionHandler = vi.fn();
		const session = {
			eventHandlers: new Set(),
			permissionHandlers: new Set([permissionHandler]),
			pendingPermissionReplies: new Map(),
		};
		const backdoor = agent as unknown as {
			_sessions: Map<string, typeof session>;
			_recordSessionNotification: (
				session: typeof session,
				notification: {
					jsonrpc: "2.0";
					method: string;
					params: Record<string, unknown>;
				},
			) => void;
		};
		backdoor._sessions = new Map([["session-1", session]]);

		backdoor._recordSessionNotification(session, {
			jsonrpc: "2.0",
			method,
			params: {
				permissionId: "legacy-permission",
				description: "dead notification route",
			},
		});

		expect(permissionHandler).not.toHaveBeenCalled();
		expect(session.pendingPermissionReplies.size).toBe(0);
		await expect(
			agent.respondPermission("session-1", "legacy-permission", "once"),
		).rejects.toThrow(
			"Permission request is not pending: legacy-permission",
		);
	},
);
```

Use the existing private-test backdoor style in that file; do not expose a new
public production method for the test.

### 2. Typed callback remains raw and answerable

In the real adapter test
`packages/core/tests/opencode-session.test.ts:780-880`, replace only the current
client-supplement assertion at lines 853-855:

```ts
expect(
	(permissionParams[0]?._acpMethod as string | undefined) ?? "",
).toBe("session/request_permission");
```

with:

```ts
expect(permissionParams[0]).not.toHaveProperty("_acpMethod");
```

Retain the assertions that exactly one handler fires, the raw adapter option IDs
are `once`/`always`/`reject`, `respondPermission` reports
`via: "sidecar-request"`, and the approved command creates the file. This is
the after proof that the working route remains typed, answerable, and raw.

Keep `packages/core/tests/permission-no-handler-warning.test.ts` unchanged. Its
three tests directly exercise `_handleAcpPermissionCallback` and prove the
client returns no fabricated answer when the host has no handler.

### 3. Native adapter conformance owns the method

Strengthen the existing
`acp_extension_creates_reports_and_closes_session_over_ext` test in
`crates/agentos-sidecar/tests/acp_extension.rs` rather than adding a second
permission implementation.

In the `AcpPermissionCallback` arm at lines 292-305, decode and assert the raw
params before returning `once`:

```rust
let params: Value =
    serde_json::from_str(&callback.params).expect("permission callback params");
assert_eq!(params["permissionId"], "perm-1");
assert_eq!(params["reason"], "Need approval");
assert_eq!(params["options"][0]["optionId"], "once");
assert!(params.get("_acpMethod").is_none());
```

After the prompt notification collection at lines 470-490, add:

```rust
assert!(notifications
    .iter()
    .all(|event| event["method"] == "session/update"));
```

The fixture at lines 1310-1323 already emits the real
`session/request_permission` request, and the existing assertion at lines
465-468 already proves the adapter receives option ID `once`. Together these
assertions pin method recognition and option translation to the native sidecar
while proving the request was not duplicated into session events.

## Before evidence and test checklist

- [ ] Add the temporary characterization expectations for both method strings.
- [ ] Run `pnpm --dir packages/core exec vitest run
  tests/session-config-routing.test.ts -t "permission callback"`; record that
  the handler fires once while `respondPermission` rejects.
- [x] Current baseline routing suites pass before implementation:
  `pnpm --dir packages/core exec vitest run
  tests/session-config-routing.test.ts tests/permission-no-handler-warning.test.ts`
  — 2 files, 11 tests passed on working-copy change `sqnqyqws`.
- [x] Source inventory confirms the only production TypeScript permission
  method interpretation is `_recordSessionNotification`, and the only
  `_acpMethod` producer is the typed callback arm.
- [x] Existing native integration already emits `session/request_permission`,
  reaches `AcpPermissionCallback`, returns `once`, and observes adapter option
  ID `once`.

The first two boxes must be checked in `docs/thin-client-migration.md` by the
implementation worker before the production deletion. The checked research
boxes above are source/baseline evidence, not completion of Item 52.

## After validation checklist

- [ ] The final table test proves both permission-shaped notifications are
  ignored and cannot manufacture a pending reply.
- [ ] `pnpm --dir packages/core exec vitest run
  tests/session-config-routing.test.ts tests/permission-no-handler-warning.test.ts`
  passes.
- [ ] `pnpm --dir packages/core exec vitest run tests/opencode-session.test.ts
  -t "supports real OpenCode permission approval through the Agent OS session API"`
  passes.
- [ ] `pnpm --dir packages/core check-types` passes.
- [ ] `cargo test -p agentos-sidecar --test acp_extension -- --nocapture` passes
  with the raw-params and event-method assertions.
- [ ] `cargo test -p agentos-client --lib` and `cargo check -p agentos-client`
  pass, preserving Rust parity without a Rust production change.
- [ ] `cargo fmt --all -- --check` and `git diff --check` pass.
- [ ] This inventory returns no production client matches:

  ```sh
  rg -n 'LEGACY_PERMISSION_METHOD|ACP_PERMISSION_METHOD|_acpMethod|request/permission|session/request_permission' \
    packages/core/src
  ```

- [ ] Update Item 52's before, after, and completion checkboxes in
  `docs/thin-client-migration.md`, then mark its work-item row `done`.

## Dependencies, risks, and boundaries

1. **Item 34 is complete.** Shared-core convergence landed as `pqpkrqpt`; it is
   no longer a blocker. Use the current shared classifier and native host seam
   rather than recreating pre-convergence logic.
2. **Item 81 overlaps only obsolete tests.** If Item 81 lands first, its deleted
   `native-sidecar/tests/acp_legacy` files stay deleted. If Item 52 lands first,
   do not modify them; either order preserves the production result.
3. **Item 44 overlaps the native catch-all.** Item 44 may remove the generic
   `AcpHostRequestCallback` round-trip. It must preserve the explicit native
   `session/request_permission` arm and generated permission callback variant.
4. **Item 53 owns structured event cleanup.** Keep real `session/update` routing
   intact here; do not expand Item 52 into removal of the broader
   `acp.session_event` compatibility shape.
5. **Item 54 owns listener error policy.** Do not change callback exception or
   warning behavior while deleting this dead invocation path.
6. **Notifications cannot answer requests.** Routing permission through events
   cannot return the original JSON-RPC response and can stall the adapter until
   timeout.
7. **The host closure remains client-side.** The sidecar cannot access a
   JavaScript/Rust application callback. The thin client may route that callback
   and retain the pending host reply; it must not recognize adapter methods or
   choose policy.
8. **Do not invent aliases.** Add adapter-specific aliases sidecar-side only
   when a real adapter and conformance fixture require one.

## Dedicated one-item JJ scope

The main sequential worker must create **one new stacked JJ revision for Item
52** when it reaches this item. Background research agents must not move `@` or
create the revision.

Bound that revision to:

- `packages/core/src/agent-os.ts`;
- `packages/core/tests/session-config-routing.test.ts`;
- `packages/core/tests/opencode-session.test.ts`;
- `crates/agentos-sidecar/tests/acp_extension.rs`;
- `docs/thin-client-migration.md`, updated only after before/after evidence; and
- this note only if a research correction is needed during implementation.

It must contain no protocol schema/generated-code, Cargo dependency, Rust
client production, browser, registry adapter, website, or lockfile changes.

Suggested revision description:

```text
refactor(core): remove legacy ACP permission notifications
```

Before advancing the stack bookmark, inspect the revision-relative diff and
confirm the only surviving client permission entrypoint begins with generated
`AcpPermissionCallback` data.
