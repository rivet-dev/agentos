# Item 54 research — surface listener failures and remove lossy conversion

Status: **implementation-ready research only**. Revalidated against
working-copy change `vsqvzlkn` (`c2a50efd`) on 2026-07-14. This note does not
change production code, tests, or the Item 54 tracker status.

Priority: **P3**. Confidence: **high**.

## Decision

Item 54 has one live TypeScript defect and one live Rust defect in the current
tree:

1. `SidecarProtocolClient.dispatchEvent` catches a host event-listener
   exception and discards it. Keep listener isolation, but emit one structured
   host-visible warning and continue routing the event.
2. Rust session creation serializes MCP entries one at a time through
   `filter_map(...ok())`, so a failed element could disappear from an otherwise
   successful request. Serialize the complete caller-supplied list once and
   return the existing typed `ClientError::InvalidArgument` on failure.

The four Rust session-state conversion losses originally covered by this item
were fixed by Item 35. Item 54 must verify those paths and tests, not reimplement
them. Item 46 is an earlier revision in the numbered stack and changes MCP
presence from a defaulted `Vec` to `Option<Vec<_>>`, but its research note
explicitly leaves the lossy `filter_map` for Item 54. Item 54 must therefore
target Item 46's final representation, preserve its omission semantics, and
replace the lossy conversion in its own dedicated revision.

Neither live defect should be moved into the sidecar:

- a TypeScript listener is executable host state that the sidecar cannot invoke
  or inspect; and
- the Rust client must serialize its typed caller input before any protocol
  request can exist.

The sidecar already owns MCP defaults, JSON validation, adapter startup, and ACP
forwarding. Item 54 adds no default, policy, protocol field, or sidecar state.

## Tracker anchors

- issue: `docs/thin-client-migration.md:100`
- work-item status: `docs/thin-client-migration.md:187`
- before/after/completion checklist: `docs/thin-client-migration.md:279`

The original issue is:

> TypeScript swallows event-listener exceptions and Rust silently drops some
> session/MCP conversion errors. Propagate failures or emit structured
> host-visible warnings.

## Complete TypeScript listener-error audit

### Live Item 54 defect

`packages/runtime-core/src/protocol-client.ts:367-395`,
`SidecarProtocolClient.dispatchEvent`, invokes matching listeners before it
resolves an event waiter or buffers the event. At lines 378-387 it currently
does this:

```ts
for (const listener of this.eventListeners) {
	if (!ownershipMatchesSelector(listener.ownership, event.ownership)) {
		continue;
	}
	try {
		listener.handler(event);
	} catch {
		// Event listeners are best-effort observers and must not break framing.
	}
}
```

The isolation is correct; the empty catch is not. Production listeners enter
through:

- `packages/runtime-core/src/native-client.ts:137-142`;
- `packages/runtime-core/src/sidecar-process.ts:423-428`; and
- AgentOS's top-level router at `packages/core/src/agent-os.ts:1337-1344`
  (`this._sidecarClient.onEvent(...)`).

An exception from any direct runtime-core consumer or the AgentOS router is
therefore invisible today.

Do **not** rethrow. `dispatchEvent` runs in inbound frame dispatch, so rethrowing
an arbitrary host callback error could break framing, skip sibling listeners,
strand a waiter, or prevent bounded-buffer delivery. The repository rule allows
the failure to be clearly logged at its failure site, and Item 54 explicitly
allows a structured warning.

### Audited paths that do not belong to Item 54

| Path | Current behavior | Disposition |
|---|---|---|
| `packages/core/src/agent-os.ts:2252-2260` session subscribers | isolates and warns | already compliant |
| `packages/core/src/agent-os.ts:2331-2344` warning callback | isolates and warns | already compliant |
| `packages/core/src/agent-os.ts:2358-2368` ACP stderr callback | isolates and warns | already compliant |
| `packages/core/src/agent-os.ts:2385-2398` ACP exit callback | isolates and warns | already compliant |
| `packages/core/src/cron/cron-manager.ts:300-305` cron listeners | isolates and warns | already compliant |
| `packages/core/src/sidecar/rpc-client.ts:825-830,873-874` stdout/stderr fan-out | listener throws can abort sibling delivery | Item 69, not a swallowed exception |
| `packages/core/src/agent-os.ts:2238-2239` legacy permission handlers | uncaught legacy route | Item 52 removes the legacy interpretation |
| `packages/core/src/agent-os.ts:1689-1705` process-exit handlers | rejection reaches the surrounding promise catch and is logged | not silent; Item 57 owns result-bearing callback parity |
| `packages/runtime-core/src/native-client.ts:81,170-200` cleanup catches | child kill/stdin-destroy race cleanup, not listener dispatch | no Item 54 edit |
| `packages/runtime-core/src/frame-stream.ts:125-127` frame listener | exception propagates | not swallowed |

This audit found no second swallowed TypeScript listener exception in the
client scope. Do not widen Item 54 into Item 52, 57, or 69.

## Complete Rust session/MCP conversion audit

### Session-state losses already fixed by Item 35

Item 35 (`nnmknwoo`, `docs/thin-client-research/item-35.md`) fixed every live
session-state shortening/collapse path that Item 54 originally referred to:

| Conversion | Original loss | Current all-or-error path | Current proof |
|---|---|---|---|
| modes | malformed present JSON/value became `None` through `.ok()` | `parse_optional_json` and `decode_optional_acp_value` | `crates/client/src/session.rs:950-962,990-997,1057-1084` |
| config options | `filter_map` silently shortened the list | indexed `parse_json_vec` and `decode_acp_values(...).collect()` | `session.rs:964-978,999-1007,1028-1055` |
| agent capabilities | malformed present value became `None` | typed decode, with only a **valid** `{}` normalized to `None` | `session.rs:1010-1015,1057-1087` |
| agent info | malformed present value became `None` | `decode_optional_acp_value` | `session.rs:990-997,1057-1084` |
| required mode/config fields | serde defaults invented empty IDs/lists | required-field decoding now fails with field context | `session.rs:1090-1105` |
| malformed wire JSON text | parse failures could be collapsed/shortened | `ClientError::AcpDecode { context, source }` | `session.rs:1108-1118`; error type at `crates/client/src/error.rs:63-69` |

The public getters use those helpers at:

- `get_session_modes`: `session.rs:1691-1694`;
- `get_session_config_options`: `session.rs:1720-1726`;
- `get_session_capabilities`: `session.rs:1729-1735`; and
- `get_session_agent_info`: `session.rs:1738-1741`.

Do not change the valid empty-capabilities normalization. It is not an error
loss. Also exclude `session.rs:439` and `session.rs:1897`: those `.ok()` calls
turn a closed permission-reply oneshot into the already modelled absent-reply
outcome; they are not JSON/session/MCP conversion.

### Live MCP loss

`crates/client/src/session.rs:1400-1420`, `AgentOs::create_session`, currently
contains:

```rust
let mcp_servers = if options.mcp_servers.is_empty() {
    None
} else {
    let values: Vec<Value> = options
        .mcp_servers
        .iter()
        .filter_map(|server| serde_json::to_value(server).ok())
        .collect();
    Some(serde_json::to_string(&values).map_err(|error| {
        ClientError::Sidecar(format!("failed to encode MCP servers: {error}"))
    })?)
};
```

The `filter_map` turns an element serialization error into successful omission
of that element. Current `McpServerConfig` variants at `session.rs:560-576`
contain only `String`, `Vec`, and `BTreeMap<String, String>`, so their derived
serializer is effectively infallible today. The defect is therefore a latent,
source-proven loss rather than a runtime failure constructible through the
current public enum. It still must be removed: a future field serializer must
not silently change the caller's server list.

The sidecar's responsibility begins after serialization:

- create paths parse `clientCapabilities` and `mcpServers` at
  `crates/agentos-sidecar-core/src/engine.rs:1419-1420,3419-3420`;
- `parse_json_text` at `engine.rs:4551-4554` returns a typed invalid-state error
  for malformed wire JSON; and
- `crates/agentos-sidecar/tests/acp_wrapper_conformance.rs:1132-1155` proves
  malformed MCP JSON is rejected before browser adapter resources are spawned.

That sidecar test stays where it is. It proves a distinct semantic boundary and
cannot test a Rust value being dropped before the request is sent.

## Exact production edits

### TypeScript: structured warning without routing failure

In `SidecarProtocolClient.dispatchEvent`, replace the empty catch with:

```ts
try {
	listener.handler(event);
} catch (error) {
	console.warn("[agent-os] sidecar event listener failed", {
		error,
		ownership: event.ownership,
		payloadType: event.payload.type,
	});
}
```

Required behavior:

- preserve per-listener synchronous isolation;
- warn once for each thrown listener invocation;
- retain the thrown value as-is, including non-`Error` throws;
- include ownership and payload type, but **not** the complete payload;
- continue to every sibling listener;
- continue waiter matching and event buffering; and
- do not close/fail the transport or reject unrelated requests.

Do not add a public `onEventListenerError` option, tracing configuration, rate
limiter, async listener contract, or client-side error queue. A small structured
`console.warn` is the existing repository pattern and keeps the client simple.

### Rust: preserve Item 46 presence and serialize all-or-error

Item 46 changes `CreateSessionOptions.mcp_servers` to
`Option<Vec<McpServerConfig>>`. Item 54 lands later, so its correct target is:

```rust
let mcp_servers = options
    .mcp_servers
    .as_ref()
    .map(|servers| {
        serde_json::to_string(servers).map_err(|error| {
            ClientError::InvalidArgument(format!(
                "failed to encode MCP servers: {error}"
            ))
        })
    })
    .transpose()?;
```

This preserves the Item 46 contract:

- `None` -> omitted wire field, so the sidecar applies its default;
- `Some(vec![])` -> present JSON `[]`;
- `Some(nonempty)` -> the complete array in caller order or a typed error.

`InvalidArgument` already exists at `crates/client/src/error.rs:47-49` and is
the correct category: explicit caller input could not be represented before a
sidecar request existed. Do not report a local serializer failure as
`ClientError::Sidecar`.

Item 46's research deliberately leaves the current `filter_map` behavior for
Item 54. Inspect its landed projection helper and put the expression above at
that final serialization boundary. The required Item 54 result is one
whole-list serialization, no per-entry `.ok()`/`filter_map`, and
`ClientError::InvalidArgument` on failure. Do not cite Item 46 as inherited
completion of the Rust half: Item 46 owns presence, while Item 54 owns
all-or-error conversion.

Do not restore today's `is_empty() -> None` behavior after Item 46. That would
erase explicit presence and regress the earlier item.

## Before evidence and after tests

### TypeScript focused test

Add coverage beside “runs the response hook before a following event is
dispatched” in `packages/runtime-core/tests/protocol-client.test.ts`. Import
`vi`, use `MemoryFrameTransport`, and register two matching listeners in order:
the first throws `new Error("listener exploded")`; the second records delivery.

One focused test should cover both immediate and buffered routing:

1. spy on `console.warn` with a no-op implementation;
2. register the throwing listener and a sibling listener;
3. start `waitForEvent` before emitting the first structured event;
4. emit it and assert the transport callback does not throw, the sibling runs
   exactly once, the waiter resolves to the same event, and the warning is
   called once with the exact message plus `{ error, ownership, payloadType:
   "structured" }`;
5. emit a second matching event with no waiter, then start `waitForEvent` and
   assert it resolves from the bounded buffer despite the listener exception;
6. assert the sibling and warning each ran exactly twice; and
7. restore the spy and dispose the client in `finally` so a failed assertion
   cannot leak the silence timer.

**Before validation:** the sibling/waiter/buffer assertions already pass, while
the warning assertion fails because the catch is empty. This directly captures
the original behavior without deliberately breaking the transport.

**After validation:** all assertions pass, proving visibility and continuity.
A sidecar test cannot throw a host JavaScript function, so this test correctly
stays in runtime-core.

### Rust focused tests and inherited evidence

Run Item 35's `acp_decode_tests` unchanged. They are the before/after proof for
the original session losses:

- `config_values_preserve_order_and_fail_at_the_original_index`;
- `malformed_present_optional_state_is_typed_while_omission_stays_none`;
- `required_mode_and_config_fields_cannot_default_to_empty_values`; and
- `malformed_json_text_uses_the_same_field_and_index_context`.

For MCP, extend Item 46's presence/request-projection test (rather than adding a
second request builder) so it proves `None`, `Some([])`, and a non-empty
two-entry list reach the wire distinctly under Item 54's final serializer. Its
non-empty fixture should contain both variants, non-empty args/env/headers, and
assert array length, caller order, and complete nested fields after JSON decode.

The current enum cannot trigger `Serialize::serialize` failure, so do not claim
a fabricated public runtime reproducer. The exact before evidence is the live
`filter_map(|server| serde_json::to_value(server).ok())`, which definitionally
discards `Err`. The exact after evidence is one complete-Vec
`serde_json::to_string` operation returning `Result`, plus a source audit that
the create-session MCP path contains no `filter_map`/`.ok()`.

Do not add a production generic serializer, a fake public MCP variant, or a
brittle repository-wide source-text test solely to manufacture an otherwise
unconstructible serde error. If Item 46 already extracted a small private
serialization seam for its presence matrix, Item 54 may add a test-only custom
`Serialize` failure there and assert `ClientError::InvalidArgument`; otherwise
the static before evidence and complete-list wire test are the honest coverage.

Before marking Item 54 complete, update its tracking checklist with exact test
names/revisions:

- **before:** the TypeScript test failed only on missing warning; Item 35 tests
  identify the prior session conversion failures; the MCP `filter_map(...ok())`
  source audit demonstrates latent element loss;
- **after:** the TypeScript listener-continuity/warning test passes; Item 35 ACP
  decode tests pass; the Item 46 request projection passes with Item 54's
  whole-list serializer and contains no `.ok()`/`filter_map`; and
- **complete:** one dedicated Item 54 JJ revision is validated and the row is
  marked `done`.

## Dependencies, risks, and non-goals

- **Numbered sequencing:** create Item 54 as one revision directly after Item
  53. Items 35 and 46 are already ancestors by then.
- **Item 35:** its `AcpDecode` changes and tests are required inherited behavior;
  do not duplicate them.
- **Item 46:** preserve its `Option<Vec<_>>` and optional nested MCP fields. Its
  research explicitly leaves lossy conversion removal to Item 54.
- **Log volume:** a listener that throws on every event will warn on every
  invocation. Rate limiting would add client policy/state and is out of scope.
- **Sensitive data:** never include the full event payload. The error object may
  itself contain caller-chosen content, which is standard for host-visible
  programming-error logging.
- **Transport health:** a listener error must not close the transport, reject a
  request, skip siblings, consume/strand a waiter, or bypass the event buffer.
- **MCP/default policy:** the sidecar continues to own all MCP defaults and ACP
  semantics. The Rust edit is serialization of explicit typed input only.
- **No protocol churn:** no schema, generated binding, sidecar, actor, or
  TypeScript public API edit is required.
- **Separate listener items:** stdout/stderr fan-out is Item 69; legacy
  permission interpretation is Item 52; result-bearing exit callbacks are Item
  57.

## Dedicated one-item JJ scope

Create exactly one Item 54 revision, directly after Item 53, with a description
such as:

```text
fix(client): surface listener and conversion failures
```

Expected paths:

```text
packages/runtime-core/src/protocol-client.ts
packages/runtime-core/tests/protocol-client.test.ts
crates/client/src/session.rs
docs/thin-client-migration.md             # Item 54 evidence/status only
```

`crates/client/src/session.rs` is required Item 54 scope after Item 46 lands; the
comment above is not conditional. Do not touch `crates/client/src/error.rs`;
`InvalidArgument` already exists. Do not touch sidecar/protocol/generated files.

## Validation

Focused gates:

```sh
pnpm --dir packages/runtime-core exec vitest run tests/protocol-client.test.ts
cargo test -p agentos-client --lib acp_decode_tests
```

If Item 46 placed MCP projection coverage in a more focused Rust test target,
run that exact target as well. Then run:

```sh
pnpm --dir packages/runtime-core check-types
cargo test -p agentos-client --lib
cargo check -p agentos-client
cargo fmt --all -- --check
git diff --check
```

Item 54 is complete only when the listener failure is visible without changing
routing semantics, the Rust create-session path contains no lossy MCP
collection, the Item 35 decode tests remain green, Item 46's presence semantics
are preserved, and the tracker records the exact before/after evidence in the
dedicated Item 54 revision.
