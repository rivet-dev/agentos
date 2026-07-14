# Item 53 research: delete the generic `acp.session_event` client branch

Status: **implementation-ready research only**. Revalidated against working-copy
change `vsqvzlkn` on 2026-07-14. This note does not modify production code,
tests, or Item 53's tracker status.

Priority: **P3**. Confidence: **high**. An exact repository inventory finds no
producer or fixture for the dotted generic event name; its only production-code
occurrence is the TypeScript compatibility consumer. Native and browser ACP
already share the schema-backed typed event path.

## Recommended fix

Delete TypeScript's handler for generic
`StructuredEvent { name: "acp.session_event" }`. Do not create that event in the
sidecar. Real ACP session notifications already use this single typed path:

```text
adapter JSON-RPC notification
  -> shared AcpSessionNotification
  -> AcpEvent::AcpSessionEvent
  -> BARE payload in dev.rivet.agent-os.acp ExtEnvelope
  -> TypeScript _handleAcpExtEvent
  -> _recordSessionNotification
  -> host session subscriber
```

Keep generic structured events for their real uses, including limit warnings,
heartbeat/liveness, unsupported guest-kernel calls, security diagnostics, and
cron errors. Item 53 removes only the invented dotted ACP convention.

## Do not confuse the two event shapes

The real schema-backed event is defined at
`crates/agentos-protocol/protocol/agent_os_acp_v1.bare:230-268`:

```bare
type AcpSessionEvent struct {
  sessionId: str
  notification: JsonUtf8
}

type AcpEvent union {
  AcpSessionEvent |
  AcpAgentStderrEvent |
  AcpAgentExitedEvent
}
```

Its generated TypeScript codec at
`packages/core/src/sidecar/agentos-protocol.ts:1076-1091,1162-1189` is required
and must remain.

The dead shape uses the generic sidecar protocol type at
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:986-997`:

```bare
type StructuredEvent struct {
  name: str
  detail: map<str><str>
}
```

No ACP schema defines `name = "acp.session_event"`, `detail.session_id`, or
`detail.notification`. That string-map convention exists only in the client
branch below.

## Original issue

History confirms this is migration residue rather than an undocumented active
producer. Commit `b1f023aae58` introduced the generic structured branch during
the TypeScript/Rust migration on 2026-04-12. Commit `b170f837541` added the
typed `_handleAcpExtEvent` route on 2026-06-16, but left the old branch in
place. Because client, protocol, and sidecar ship in lockstep with no backward
compatibility guarantee, retaining both representations has no compatibility
value.

`AgentOs._handleSidecarEvent` in
`packages/core/src/agent-os.ts:2372-2439` correctly handles extension events,
cron dispatch, and `limit_warning`. It then contains this compatibility branch
at current lines 2416-2438:

```ts
if (event.payload.name !== "acp.session_event") {
	return;
}

const sessionId = event.payload.detail.session_id;
const session = sessionId ? this._sessions.get(sessionId) : undefined;
if (!session) {
	return;
}

const notificationText = event.payload.detail.notification;
if (typeof notificationText !== "string") {
	return;
}

try {
	this._recordSessionNotification(
		session,
		toJsonRpcNotification(JSON.parse(notificationText)),
	);
} catch (error) {
	console.warn("invalid ACP session event from sidecar", error);
}
```

The branch duplicates session lookup, JSON parsing, filtering, and subscriber
delivery already performed by the typed `AcpSessionEvent` arm in
`AgentOs._handleAcpExtEvent` at lines 2479-2512. Carrying both shapes makes the
client own undocumented transport compatibility and creates a possible double
delivery path without serving any current producer.

The tracker entries are currently at
`docs/thin-client-migration.md:99,186,278`.

## Exact files, symbols, and callers

| Role | Current source | Item 53 action |
| --- | --- | --- |
| Dead compatibility consumer | `packages/core/src/agent-os.ts:2372-2439`, `AgentOs._handleSidecarEvent` | Delete only lines 2416-2438. |
| Authoritative TypeScript consumer | `packages/core/src/agent-os.ts:2479-2512`, `AgentOs._handleAcpExtEvent` | Keep; its `AcpSessionEvent` arm is the sole TypeScript ACP session-event route afterward. |
| TypeScript event wiring | `packages/core/src/agent-os.ts:1332-1343`, constructor registration of `SidecarProcess.onEvent` | Keep; it forwards every VM-scoped wire event to `_handleSidecarEvent`. |
| Generic/extension wire mapping | `packages/runtime-core/src/event-buffer.ts:45-53,403-413`, `LiveSidecarEventPayload` and `fromGeneratedEventPayload` | Keep; this is a shape-only mapping and contains no ACP name translation. |
| ACP event schema and codec | `crates/agentos-protocol/protocol/agent_os_acp_v1.bare:230-268`; `packages/core/src/sidecar/agentos-protocol.ts:1076-1091,1162-1202` | Keep `AcpSessionEvent` and `AcpEvent`. |
| Generic structured schema | `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:986-997` | Keep; non-ACP structured producers still use it. |
| Shared ACP producer | `crates/agentos-sidecar-core/src/engine.rs:806-845`, `encode_session_notification` and `push_session_notification_batch` | Keep; it emits only `AcpEvent::AcpSessionEvent`. |
| Native wrapper | `crates/agentos-sidecar/src/acp_extension.rs:457-490,1884-1892` | Keep; it wraps the typed payload in an ACP `ExtEnvelope`. |
| Browser wrapper | `crates/agentos-sidecar-browser/src/lib.rs:207-229` | Keep; it drains the same shared-core typed event queue. |
| Rust client parity consumer | `crates/client/src/agent_os.rs:639-691,736-764`, `spawn_acp_event_pump` and `deliver_acp_ext_event` | Keep; it decodes typed extension events and explicitly ignores generic `StructuredEvent`. |
| Focused TypeScript regression | `packages/core/tests/session-event-ordering.test.ts`, `createTrackedAgent` and `AgentOs session event ordering` | Add the before/after private-route regression described below. |
| Stale guidance | `crates/CLAUDE.md:96` | Replace `acp.session_event` with `the typed AcpSessionEvent stream` unless Item 51 already did so. |

## Exact producer inventory

### Exact dotted-name inventory

Run:

```sh
rg -n 'acp\.session_event' . \
  --glob='!docs/thin-client-migration.md' \
  --glob='!docs/thin-client-research/**' \
  --glob='!.claude/worktrees/**' \
  --glob='!node_modules/**' \
  --glob='!target/**'
```

At the inspected revision it returns exactly:

- `packages/core/src/agent-os.ts:2416`, the dead consumer; and
- `crates/CLAUDE.md:96`, stale prose that names the nonexistent shape.

There is no Rust or TypeScript producer, protocol schema, generated codec,
adapter fixture, or test fixture for the generic dotted name.

### Authoritative shared-core producer

The typed event is sidecar-owned:

1. `AcpSessionNotification` in
   `crates/agentos-sidecar-core/src/behavior.rs:455-473` represents a decoded,
   session-scoped adapter notification and rejects malformed wire JSON.
2. `AcpSyntheticSessionUpdate::notification` and
   `apply_successful_session_request` at current lines 475-512 create the same
   `session/update` JSON shape when an adapter accepted a mode/config change but
   omitted its update notification.
3. `AcpCore::encode_session_notification` in
   `crates/agentos-sidecar-core/src/engine.rs:806-818` serializes that value into
   `AcpEvent::AcpSessionEvent`; `push_session_notification_batch` at lines
   821-835 commits those typed events.

Native delivery then uses:

- `crates/agentos-sidecar/src/acp_extension.rs:457-478` to encode each committed
  `AcpEvent` and wrap it with `ctx.ext_event_wire`; and
- `deliver_event` at lines 1884-1892 to send the extension frame live or retain
  it in the request's event batch.

Browser delivery uses the same `AcpCore` event queue and shared BARE codec at
`crates/agentos-sidecar-browser/src/lib.rs:185-229`. It never maps ACP updates
to generic `StructuredEvent`.

The runtime-core transport preserves those two wire variants independently:
`packages/runtime-core/src/event-buffer.ts:403-413` maps
`StructuredEvent -> { type: "structured" }` and
`ExtEnvelope -> { type: "ext" }` without translating names or payloads. The
Rust client independently confirms parity at
`crates/client/src/agent_os.rs:639-691,736-764`: its event pump decodes ACP only
from `ExtEnvelope` and ignores every generic `StructuredEvent`. Therefore there
is no hidden transport conversion that could fabricate the dotted name for the
TypeScript-only branch.

### Existing sidecar tests own producer behavior

`crates/agentos-sidecar/tests/acp_extension.rs` already proves expected ACP
updates use typed extension framing:

- create/bootstrap is decoded by the strict helper at lines 350-374;
- prompt updates decode `EventPayload::ExtEnvelope` and
  `AcpEvent::AcpSessionEvent` at lines 470-490;
- synthesized mode/config updates use the same strict helper at lines 492-543;
  and
- `decode_single_acp_session_event` at lines 1087-1098 requires exactly one
  extension event in the ACP namespace and the typed BARE variant.

Shared semantic parity is also covered by
`crates/agentos-sidecar-core/tests/acp_conformance.rs:330-339` and native/browser
wrapper decoding in
`crates/agentos-sidecar/tests/acp_wrapper_conformance.rs:1772-1795`.
The wrapper's inbound-request regression at lines 2198-2211 separately proves
agent-to-host requests never become `AcpSessionEvent`s.

No client test needs to be moved into the sidecar for Item 53. Production,
synthetic updates, and framing already have sidecar-owned coverage; the new
TypeScript regression covers the remaining client responsibility—selecting the
typed event route and ignoring an undocumented generic name.

### Legitimate generic structured producers

Do not remove the generic protocol type. Current unrelated producers include:

- `limit_warning` at `crates/native-sidecar/src/stdio.rs:179-188,293-319`;
- `heartbeat` at `crates/native-sidecar/src/stdio.rs:767-780`;
- unsupported guest-kernel operation diagnostics at
  `crates/native-sidecar-core/src/frames.rs:419-437`;
- security/audit bridge records at
  `crates/native-sidecar/src/service.rs:4489-4517`; and
- `cron_dispatch_error` at
  `crates/native-sidecar-browser/src/wire_dispatch.rs:2381-2390`.

None is an ACP session notification.

## Exact production edit

### `packages/core/src/agent-os.ts`

Delete lines 2416-2438—the complete `"acp.session_event"` block shown above.
Leave `_handleSidecarEvent` with these responsibilities:

```ts
private _handleSidecarEvent(event: SidecarEvent): void {
	if (event.payload.type === "ext") {
		this._handleAcpExtEvent(event.payload.envelope);
		return;
	}
	if (event.payload.type === "cron_dispatch") {
		// Existing cron mapping remains unchanged.
		return;
	}
	if (event.payload.type !== "structured") {
		return;
	}
	if (event.payload.name === "limit_warning") {
		this._handleLimitWarning(event.payload.detail);
	}
}
```

`SidecarEvent` above is shorthand for the method's existing inferred parameter
type; do not change its real signature merely to match this outline.

Keep unchanged:

- `toJsonRpcNotification`, because the typed ACP arm still uses it;
- `_recordSessionNotification`, the host-side subscriber router;
- `_handleAcpExtEvent`, `decodeAcpEvent`, and
  `ACP_EXTENSION_NAMESPACE`;
- `shouldDispatchToSessionEventHandlers` and public subscription semantics;
- the generic `StructuredEvent` protocol and runtime mappings; and
- all Rust client ACP event routing, which already consumes the typed variant.

No sidecar production edit is required.

### `crates/CLAUDE.md`

The current sentence at line 96 says synthetic updates keep
`getSessionState()`, `acp.session_event`, and the TypeScript session API in sync.
Replace only the nonexistent term with the real transport description:

```text
... so getSessionState(), the typed AcpSessionEvent stream, and the TypeScript
session API stay agent-agnostic ...
```

Item 51 owns broader guidance cleanup. If its earlier stacked revision already
rewrites this sentence, Item 53 must not touch `crates/CLAUDE.md` again.

## Exact before and after test

Add the regression to
`packages/core/tests/session-event-ordering.test.ts`, because that file already
owns TypeScript event subscription and typed extension routing.

### Test backdoor edit

Extend the `createTrackedAgent` private intersection type after its existing
`_handleAcpExtEvent` declaration:

```ts
_handleSidecarEvent(event:
	| {
			payload: {
				type: "structured";
				name: string;
				detail: Record<string, string>;
			};
		}
	| {
			payload: {
				type: "ext";
				envelope: { namespace: string; payload: Uint8Array };
			};
		}): void;
```

Do not add public visibility to production code for this test.

### Failing-before regression and characterization

The lasting test below is the exact failing-before regression: on the current
parent its first `expect(seen).toEqual([])` fails because the client appends
`"structured"`. That failure proves the undocumented generic shape is active.
After the branch deletion the same expectation passes, while the typed event
still appends `"typed"`.

Before deleting the production branch, add a temporary test named:

```ts
"routes the obsolete structured ACP shape as well as the typed event"
```

Register one session event subscriber, then drive `_handleSidecarEvent` with:

```ts
agent._handleSidecarEvent({
	payload: {
		type: "structured",
		name: "acp.session_event",
		detail: {
			session_id: SESSION_ID,
			notification: JSON.stringify(
				createSessionUpdateNotification("structured"),
			),
		},
	},
});
expect(seen).toEqual(["structured"]);

agent._handleSidecarEvent({
	payload: {
		type: "ext",
		envelope: {
			namespace: ACP_EXTENSION_NAMESPACE,
			payload: encodeAcpEvent({
				tag: "AcpSessionEvent",
				val: {
					sessionId: SESSION_ID,
					notification: JSON.stringify(
						createSessionUpdateNotification("typed"),
					),
				},
			}),
		},
	},
});
expect(seen).toEqual(["structured", "typed"]);
```

Run that test before the deletion and record the pass. It proves the obsolete
generic shape is currently an active second route, not merely unreachable code.

### Lasting after regression

After deleting the branch, rename the test to:

```ts
"ignores the obsolete structured ACP shape and routes the typed event"
```

Retain the same fixture, but change the two expectations to:

```ts
expect(seen).toEqual([]); // after obsolete structured input
// send the typed ExtEnvelope
expect(seen).toEqual(["typed"]);
```

This single test proves both halves of the intended behavior: no generic ACP
compatibility and no regression in the authoritative typed route.

### Existing client regressions to retain

Run unchanged:

- `packages/core/tests/session-route-registration.test.ts`, which proves create
  and resume install their routes before typed events arrive; and
- `packages/core/tests/cross-session-event-isolation.test.ts`, which proves a
  typed event is correlated only to its named host route.

The threat-model comment at the top of
`cross-session-event-isolation.test.ts` incorrectly calls the trusted sidecar
event stream untrusted. That wording should be corrected by Item 51's guidance
cleanup (or tracked separately), not bundled into this dead-branch revision.

## Before evidence checklist

- [x] Exact source inventory finds no `acp.session_event` producer, schema,
  generated variant, or fixture; only the client consumer and stale CLAUDE prose
  remain outside migration/research docs.
- [x] Existing focused client baseline passes at working-copy change
  `vsqvzlkn`:
  `pnpm --dir packages/core exec vitest run
  tests/session-event-ordering.test.ts tests/session-route-registration.test.ts
  tests/cross-session-event-isolation.test.ts` — 3 files, 9 tests passed.
- [x] Existing native/shared-core tests identify
  `AcpEvent::AcpSessionEvent` extension envelopes as the real producer shape.
- [ ] Add and run the temporary characterization test; record that both the
  obsolete generic input and typed input invoke the subscriber.

The final unchecked box must be recorded in `docs/thin-client-migration.md` by
the implementation worker before deleting the production branch. Research
checks alone do not complete Item 53.

## After validation checklist

- [ ] The lasting event-ordering test ignores the generic dotted name and
  delivers exactly one callback for the typed envelope.
- [ ] `pnpm --dir packages/core exec vitest run
  tests/session-event-ordering.test.ts tests/session-route-registration.test.ts
  tests/cross-session-event-isolation.test.ts` passes.
- [ ] `pnpm --dir packages/core check-types` passes.
- [ ] `cargo test -p agentos-sidecar --test acp_extension` passes.
- [ ] `cargo test -p agentos-sidecar-core --test acp_conformance` passes.
- [ ] If adjacent ACP work landed in the same stack, run the expensive parity
  gate: `cargo test -p agentos-sidecar --test acp_wrapper_conformance`.
- [ ] `git diff --check` passes.
- [ ] The exact dotted-name search returns no production-code match. If Item 51
  already corrected `crates/CLAUDE.md`, it returns no match outside tracker and
  research documents.
- [ ] Item 53's before, after, and completion boxes are checked in
  `docs/thin-client-migration.md`, and its work-item row is marked `done` only
  after the dedicated revision ID is known.

## Dependencies, risks, and boundaries

1. **Item 34 is complete.** Shared native/browser ACP convergence landed as
   `pqpkrqpt`; its one typed core is the basis for deleting compatibility.
2. **Items 51 and 52 are stack predecessors.** The sequential worker should
   create Item 53 only after both are sealed. Item 51 may already resolve the
   CLAUDE wording; Item 52 shrinks `_recordSessionNotification` and may move the
   line anchors above, but does not replace the typed event route.
3. **Item 54 owns error policy.** Do not change catches, warnings, listener
   exception handling, or malformed-event behavior in this revision.
4. **Do not delete the generated lookalike.** `AcpSessionEvent`, `AcpEvent`,
   extension framing, and their tests are the real protocol.
5. **Do not delete generic structured events.** `limit_warning`, heartbeat,
   audit/diagnostic, and cron paths remain legitimate.
6. **Do not add a fallback producer.** Translating typed ACP events into a
   generic detail map in runtime-core or sidecar would preserve the duplication
   this item removes.
7. **Sidecar tests retain sidecar behavior.** Client tests after this change
   cover only host route selection, registration, ordering, and correlation.
8. **Trust boundary wording matters.** The sidecar is trusted; session-ID
   correlation is correctness and isolation between host subscriptions, not
   defense against a malicious sidecar.

## Dedicated one-item JJ scope

The main sequential worker must create **one new stacked JJ revision for Item
53** after Item 52 is sealed. Background research agents must not move `@` or
create the revision.

Expected revision paths:

```text
packages/core/src/agent-os.ts
packages/core/tests/session-event-ordering.test.ts
crates/CLAUDE.md                                             # omit if Item 51 fixed it
docs/thin-client-migration.md
```

No sidecar production/test, ACP schema, generated codec, runtime-core, Rust
client, dependency, lockfile, registry, or website change is expected.

Suggested revision description:

```text
refactor(core): remove obsolete ACP structured events
```

Before advancing the stack bookmark, inspect the revision-relative diff and
confirm `AgentOs` receives session notifications only through the generated
`AcpSessionEvent` extension route.

## Ordered edit sequence

1. After Item 52 is sealed, run `pwd` and `jj log -r @`, then create Item 53's
   single stacked revision with the description above. Do not move the shared
   working copy to another revision.
2. In `packages/core/tests/session-event-ordering.test.ts`, add the private
   `_handleSidecarEvent` test backdoor and the temporary characterization test.
   Run that one file and record the passing-before result showing both
   `"structured"` and `"typed"` deliveries.
3. Rename the same test to the lasting `ignores ... and routes ...` name and
   change its expectations to `[]` after the generic event and `["typed"]`
   after the extension event.
4. Delete only `packages/core/src/agent-os.ts:2416-2438`. Do not touch the
   typed handler, generated codecs, runtime-core event mapping, Rust client, or
   either sidecar wrapper.
5. If Item 51 has not already fixed it, update only the obsolete term in
   `crates/CLAUDE.md:96` as specified above.
6. Run the focused TypeScript tests, core typecheck, native/shared ACP tests,
   exact dotted-name inventory, and `git diff --check` from the validation
   checklists.
7. Update all three Item 53 tracker rows with the named before test, named
   lasting after test, validation commands, and the dedicated JJ revision ID;
   mark the item `done` only after every required gate passes.
