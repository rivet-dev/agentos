# Item 44 research: reject unknown ACP host methods in the sidecar

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 44 tracker status.

Refreshed against the shared working tree on 2026-07-14. Priority: **P2**.
Confidence: **high**.

## Recommendation

Return the shared JSON-RPC method-not-found response from the native sidecar as
soon as an adapter sends an inbound method that is not one of the native ACP
filesystem, terminal, or permission methods. Delete the generic
`AcpHostRequestCallback` request/response variants and both client fallback
branches.

There is no usable host-extension API to preserve. TypeScript always returns a
null response, Rust always returns `None`, and the browser sidecar already
returns `-32601` without involving a client. The current native round-trip can
therefore change only latency and failure mode, never the successful result.

Keep the typed `AcpPermissionCallback`. Permission decisions are the one real
host-side operation in this path, and both clients have an explicit permission
handler for it. Also keep the native sidecar's internal
`NativeCoreCommand::InboundRequest` exchange: it is how the synchronous shared
ACP core reaches the sidecar's async filesystem, terminal, and permission
implementations. Item 44 removes the sidecar-to-client fallback, not that
sidecar-internal bridge.

All producers and consumers of the dead callback are in this repository, both
clients decline it, the protocol ships in lockstep, and the canonical
sidecar-owned response already exists and is tested.

## Original issue and exact current flow

Line numbers below are from the shared working tree named above and may move as
earlier stack items land.

An adapter-to-host JSON-RPC request is classified and answered by the shared ACP
core. `answer_inbound_request` in
`crates/agentos-sidecar-core/src/engine.rs:4525-4532` calls the host hook and
writes its returned JSON back to the adapter; its resumable exchange states call
that helper at lines 1672, 1818, 2187, and 2327. The blocking exchange path does
the same at `crates/agentos-sidecar-core/src/json_rpc.rs:93-98`.

The default host implementation is already correct. At
`crates/agentos-sidecar-core/src/host.rs:81-90`, it calls
`unsupported_inbound_request_response`, which produces this canonical response
at `crates/agentos-sidecar-core/src/behavior.rs:49-65`:

```json
{
  "jsonrpc": "2.0",
  "id": "the original request id",
  "error": {
    "code": -32601,
    "message": "method not found: host/not-found",
    "data": { "method": "host/not-found" }
  }
}
```

`BrowserAcpHost` implements `AcpHost` at
`crates/agentos-sidecar-browser/src/acp_host.rs:177-357` without overriding
`handle_inbound_request`, so it already uses that response. The production
browser-wrapper test is declared at
`crates/agentos-sidecar/tests/acp_wrapper_conformance.rs:84-93`; its
`run_browser_create` fixture at lines 2134-2215 proves that an inbound
`host/read` request receives `-32601`, preserves its ID and method, does not
complete the pending handshake, and never becomes a session event.

Native ACP must override the hook because supported agent-to-host methods need
native facilities. `NativeCoreHost::handle_inbound_request` at
`crates/agentos-sidecar/src/acp_extension.rs:194-204` sends an internal
`NativeCoreCommand::InboundRequest`; the async broker handles it at lines
1151-1172. `build_inbound_response` at lines 1970-2075 then correctly routes:

- `session/request_permission` to the typed client permission callback;
- `fs/read`, `fs/write`, and their aliases to the native filesystem handler;
- `terminal/*` to the native terminal handler; and
- every other method to `forward_inbound_host_request`.

That last arm is the defect:

```rust
_ => forward_inbound_host_request(ctx, session_id, message, &id)?,
```

`forward_inbound_host_request` at
`crates/agentos-sidecar/src/acp_extension.rs:2828-2859` serializes the complete
request into `AcpHostRequestCallback`, calls the client through
`ctx.invoke_callback(..., Duration::from_secs(120))`, decodes a generic JSON
response, and validates its ID. Only a decoded null/incorrect callback variant
is converted to the shared `-32601` response.

More precisely, only a successfully decoded callback response whose generic
`response` field is absent (or whose union variant is not the generic host
variant) falls back to `-32601`. A missing transport, disconnected client,
write timeout, response timeout, malformed BARE/JSON response, or mismatched
JSON-RPC ID propagates as a sidecar error instead of method-not-found.

### Authoritative wait chain

The complete native wait is:

1. `answer_inbound_request` in
   `crates/agentos-sidecar-core/src/engine.rs:4525-4532` calls
   `NativeCoreHost::handle_inbound_request` while the core is already waiting
   for the adapter's enclosing create/resume/prompt response.
2. `NativeCoreHost::exchange` in
   `crates/agentos-sidecar/src/acp_extension.rs:137-151` sends
   `NativeCoreCommand::InboundRequest` and blocks on its one-shot sync channel.
3. The async native broker arm at
   `crates/agentos-sidecar/src/acp_extension.rs:1151-1172` calls
   `build_inbound_response`; its unknown arm calls
   `forward_inbound_host_request`.
4. `ExtensionContext::invoke_callback` delegates through
   `ExtensionSnapshot::invoke_callback` at
   `crates/native-sidecar/src/extension.rs:289-299,370-376`.
5. `SharedSidecarRequestClient::invoke` at
   `crates/native-sidecar/src/state.rs:260-283` allocates a request ID, sends a
   `SidecarRequestFrame`, and requires matching ownership and request ID.
6. The stdio transport registers the waiter before emitting the frame, then
   waits for the matching response until the supplied deadline at
   `crates/native-sidecar/src/stdio.rs:1781-1933`.

Unlike the retained permission path, this generic callback uses
`invoke_callback`, not `invoke_callback_cancellable`, and has no entry in
`permission_waits`. An interrupt therefore has no callback-specific
cancellation handle. Item 34's per-owner cores prevent this wait from taking a
different owner down with it, but the exact owner's ACP transition and adapter
response remain blocked until the callback answers or the 120-second deadline
fails.

Both clients guarantee that the callback has no useful result:

- TypeScript `_handleAcpExtSidecarRequest` at
  `packages/core/src/agent-os.ts:2834-2876` decodes the callback and its
  `AcpHostRequestCallback` arm always sends
  `AcpHostRequestCallbackResponse { response: null }`.
- Rust `handle_acp_ext_callback` at
  `crates/client/src/agent_os.rs:1316-1320` always sends the equivalent
  `response: None`.

No public registration method, handler map, or method-keyed extension API feeds
either branch. Registered host tools are a separate explicit tool surface and
must not make arbitrary ACP JSON-RPC methods callable.

The result is a client round-trip with a 120-second failure window solely to
obtain the answer the sidecar already knows. A disconnected, blocked, malformed,
or mismatched client response can turn an ordinary unsupported method into a
timeout or sidecar error. Native and browser also behave differently even
though both ultimately reject the same request.

## Exact production edits

### `crates/agentos-sidecar/src/acp_extension.rs`

1. Replace the unknown-method arm in `build_inbound_response` with the shared
   response:

   ```rust
   _ => unsupported_inbound_request_response(message),
   ```

   Do not add a second method-not-found constructor. The shared helper preserves
   the request ID and returns the same error shape as the browser host.

2. Delete `forward_inbound_host_request` completely.

3. Remove the now-unused `AcpHostRequestCallback` import.

4. Delete `json_rpc_id_label` and `parse_json_text`. Repository search shows
   both helpers exist only for the deleted generic response and ID validation.

5. Remove the `AcpHostRequestCallbackResponse` arm from
   `permission_callback_reply`. After the protocol cleanup, the callback
   response union contains only `AcpPermissionCallbackResponse`; simplify the
   function to unwrap that one variant and preserve the existing missing-reply
   default of `reject`.

Do not delete `NativeCoreHost::handle_inbound_request`,
`NativeCoreCommand::InboundRequest`, the async broker arm, or any supported
filesystem/terminal/permission case. Moving the supported-method list into the
synchronous host just to avoid the internal exchange would duplicate routing
policy and is outside this item.

### `crates/agentos-sidecar-core/src/codec.rs`

Delete the unused public `encode_callback` helper at current lines 25-28 and
remove `AcpCallback` from the line-6 import. No production caller uses this
shared encoder: browser has no client callback transport, while native owns its
permission callback encoding inside `acp_extension.rs`. Leaving it would retain
a generic callback API after the generic callback itself is gone.

### `crates/agentos-protocol/protocol/agent_os_acp_v1.bare`

At current lines 270-304, delete these dead wire structs:

```text
AcpHostRequestCallback
AcpHostRequestCallbackResponse
```

Remove their arms from `AcpCallback` and `AcpCallbackResponse`, leaving the
typed permission variants as single-arm unions:

```bare
type AcpCallback union {
  AcpPermissionCallback
}

type AcpCallbackResponse union {
  AcpPermissionCallbackResponse
}
```

The TypeScript BARE generator accepts these single-arm unions. Keeping the
permission arm at tag zero also leaves the permission callback's encoded bytes
unchanged. Do not retain a dead reserved arm: the project has no protocol
backward-compatibility guarantee and clients/sidecar ship in lockstep.

### Generated TypeScript protocol

Run:

```sh
pnpm --dir packages/core build:agentos-protocol
```

This regenerates `packages/core/src/sidecar/agentos-protocol.ts`. Do not hand
edit it. The affected current generated block is lines 1223-1407. It should no
longer export readers, writers, types, or union cases named
`AcpHostRequestCallback` or
`AcpHostRequestCallbackResponse`.

Rust protocol code is generated into Cargo `OUT_DIR` by
`crates/agentos-protocol/build.rs`; there is no checked generated Rust source to
edit.

### `packages/core/src/agent-os.ts`

Delete the `case "AcpHostRequestCallback"` branch from
`_handleAcpExtSidecarRequest`. With the regenerated one-variant type, keep only
the typed permission decoding, handler call, and response encoding.

It is safe, and simpler, to remove the switch and read the sole generated
variant's `val` directly. Do not add a generic JSON request handler or a client
method-not-found fallback. An unknown adapter method must never reach this
function after the sidecar change.

### `crates/client/src/agent_os.rs`

Remove the `AcpHostRequestCallbackResponse` import and the
`AcpCallback::AcpHostRequestCallback` match arm. Destructure the sole
`AcpPermissionCallback` variant and retain the current typed permission routing
and response.

Do not remove the ACP extension callback handler itself. Rust still needs it to
answer explicit permission requests.

### Existing guidance

No guidance edit is required for Item 44. `packages/core/CLAUDE.md:57` already
states that native filesystem and terminal methods execute in the sidecar,
unknown methods return `-32601`, and clients must not recreate a generic ACP
request dispatcher. The implementation should make that existing rule true.

## Before validation

Use the existing native integration path in
`crates/agentos-sidecar/tests/acp_extension.rs:186-281`, not a mock of
`unsupported_inbound_request_response`. The JavaScript adapter is
`terminal_adapter_script` at lines 1403-1507.

`acp_terminal_requests_stay_inside_sidecar` already has an adapter fixture that
runs supported terminal methods and then sends:

```json
{"jsonrpc":"2.0","id":105,"method":"host/not-found","params":{}}
```

Its current host callback handler explicitly decodes
`AcpHostRequestCallback`, asserts the method is `host/not-found`, returns
`response: None`, and thereby allows the prompt to receive `-32601`. Despite
the test's message saying the request must not reach the client, the test
currently requires that client callback to complete.

Before changing production code, make the characterization explicit:

1. capture an `Arc<AtomicUsize>` in the callback handler;
2. increment it after decoding the `host/not-found` callback;
3. keep returning `response: None`; and
4. assert the count is exactly one after the prompt completes.

Run that test against Item 44's parent revision and record its name and parent
revision in the tracking checklist. It deterministically proves the generic
host callback was emitted. The production call to `invoke_callback` proves the
request is synchronously waiting for that response; do not add a flaky elapsed
time assertion merely to restate the 120-second code path.

The suite currently passes on the parent (`acp_extension_suite`, plus its bridge
support test). The browser method-not-found test and both shared-core canonical
response/exchange tests also pass. Record the dedicated callback count after
adding it; the existing pass alone does not quantify the callback.

## After validation

In the same integration test, replace the characterization handler with one
that increments the counter and immediately returns
`agentos_native_sidecar::SidecarError::InvalidState("unexpected ACP client callback".into())`.
The adapter's `host/not-found` request must still complete and the counter must
remain zero. This makes a regression fail immediately instead of waiting 120
seconds; a bare no-handler setup would prove completion but not explicitly
prove callback absence.

Extend `terminal_adapter_script` to return the complete unknown-method response
inside its prompt result, then assert:

- response ID is `105`;
- `error.code` is `-32601`;
- `error.message` is `method not found: host/not-found`;
- `error.data.method` is `host/not-found`; and
- the client callback count is zero.

Retain the existing terminal output/exit/truncation assertions in the same test.
They prove that supported native host methods still use the sidecar handlers
while only the unknown fallthrough changed. Rename the test to something
explicit such as
`acp_terminal_and_unknown_host_requests_stay_inside_sidecar` if useful.

The existing `acp_extension_creates_reports_and_closes_session_over_ext` test
must continue to exercise a real typed permission callback. Remove only its now
impossible generic-host match arm. Do the same in
`install_default_acp_callback_handler`.

Add small protocol round-trip coverage so deleting the dead arm cannot
accidentally damage the retained callback:

- in `crates/agentos-protocol/tests/roundtrip.rs`, round-trip an
  `AcpPermissionCallback` and `AcpPermissionCallbackResponse` through BARE;
- in `packages/core/tests/agentos-protocol.test.ts`, round-trip the same
  permission request/response with the generated TypeScript ACP codec. This is
  the ACP codec test file; `generated-protocol.test.ts` exercises the separate
  runtime wire-frame generator and is not the authoritative home for this
  callback union.

Retain and run these existing regressions:

- `crates/agentos-sidecar-core/src/behavior.rs` verifies canonical method-not-
  found identity and data;
- `crates/agentos-sidecar-core/src/json_rpc.rs` verifies inbound requests get a
  response rather than entering the notification stream;
- `browser_wrapper_rejects_inbound_host_requests_during_create` verifies the
  already-correct browser behavior;
- Rust client unit
  `malformed_permission_callback_params_are_not_replaced_with_empty_json`
  verifies the retained client callback decoder;
- TypeScript permission routing tests verify the retained host permission
  handler behavior.

## Validation commands

Run after Item 44 has its own child `jj` revision:

```sh
pnpm --dir packages/core build:agentos-protocol
cargo fmt --all -- --check
cargo test -p agentos-protocol
cargo test -p agentos-sidecar-core
cargo test -p agentos-sidecar --test acp_extension -- --nocapture
cargo test -p agentos-sidecar --test acp_wrapper_conformance \
  browser_wrapper_rejects_inbound_host_requests_during_create -- --nocapture
cargo test -p agentos-client --lib
cargo check -p agentos-client
pnpm --dir packages/core exec vitest run tests/generated-protocol.test.ts \
  tests/agentos-protocol.test.ts tests/session-config-routing.test.ts \
  tests/permission-no-handler-warning.test.ts
pnpm --dir packages/core check-types
git diff --check
```

Finish with a source inventory. It must return no matches:

```sh
rg -n "AcpHostRequestCallback|forward_inbound_host_request" crates packages
```

Do not claim completion if the native integration test was skipped or if the
test still installs a null-returning generic callback.

## Dependencies, risks, and boundaries

- **Stack order:** implement after Item 43 and before Item 45 in one dedicated
  child revision.
- **Item 52:** preserve the typed `AcpPermissionCallback` route and its tag-zero
  encoding; that item later simplifies only permission routing semantics.
- **ACP convergence:** retain `NativeCoreCommand::InboundRequest`; it is the
  synchronous-core/async-sidecar bridge for supported native host methods, not
  the dead client fallback.

- **Vendor-specific inbound methods:** an adapter that emits an undocumented
  method still receives the same `-32601` it receives today from both shipped
  clients, but without the delay. If host extensions are added later, design an
  explicit method registration API and forward only registered methods; do not
  restore catch-all client dispatch.
- **Permission regression:** do not route `session/request_permission` through
  the unknown arm and do not remove the typed callback transport. The retained
  permission integration and client tests are the guard.
- **Supported filesystem/terminal regression:** keep their cases ahead of the
  unknown arm and retain the current integration assertions.
- **Protocol shape:** deleting union tag one is safe under lockstep releases,
  and permission remains tag zero. Regenerate TypeScript and compile both Rust
  and TypeScript consumers in the same revision.
- **False promptness checks:** wall-clock assertions are noisy. A callback
  handler that fails the test if invoked, combined with successful prompt
  completion, proves the request no longer waits on the client.
- **Scope creep:** do not invent a standard terminal protocol in this item.
  Native terminal methods are already sidecar-owned and covered; Item 44 is
  only the unsupported-method fallback.

## Dedicated Item 44 revision scope

Create a new stacked `jj` child only after Item 43 is sealed. Suggested
description:

```text
refactor(acp): reject unknown host methods in sidecar
```

Expected bounded path set:

- `crates/agentos-sidecar/src/acp_extension.rs`
- `crates/agentos-sidecar/tests/acp_extension.rs`
- `crates/agentos-sidecar-core/src/codec.rs`
- `crates/agentos-protocol/protocol/agent_os_acp_v1.bare`
- `crates/agentos-protocol/tests/roundtrip.rs`
- `crates/client/src/agent_os.rs`
- `packages/core/src/agent-os.ts`
- `packages/core/src/sidecar/agentos-protocol.ts` (generated)
- `packages/core/tests/agentos-protocol.test.ts`
- `docs/thin-client-migration.md` (checklists/status only after validation)

No Cargo or pnpm lockfile should change. No sidecar-core behavior file or browser
production file should change; the only shared-core edit is deletion of its
unused codec helper. Before describing/sealing the revision, use `jj diff` to
ensure unrelated shared-working-copy changes are not included, then record the
before test, after test, validation commands, revision ID, and completion status
in the Item 44 tracking row.

## Proposed small diff sequence

Keep all steps in the one dedicated Item 44 `jj` revision, but make the edits in
this order so each behavior change is easy to inspect:

1. **Characterize the parent:** add the callback counter to
   `acp_terminal_requests_stay_inside_sidecar`, run `acp_extension_suite`, and
   record that the unknown request completes only after exactly one generic
   client callback.
2. **Move the decision:** change only the unknown arm of
   `build_inbound_response` to the shared method-not-found helper; change the
   same integration fixture to fail on any callback and assert the full
   `-32601` response plus a zero callback count. Re-run the suite before doing
   protocol cleanup.
3. **Delete the dead wire surface:** remove the generic callback request and
   response variants from the BARE schema, regenerate TypeScript, and delete
   the now-impossible TypeScript/Rust client match arms. Add the Rust and
   TypeScript permission callback round trips in the same step.
4. **Prune unreachable helpers:** delete
   `forward_inbound_host_request`, `json_rpc_id_label`, the native-only
   `parse_json_text`, and the unused shared-core `encode_callback`; simplify
   the remaining permission response match.
5. **Seal parity:** run the focused native, browser, protocol, Rust-client, and
   TypeScript permission suites, then the zero-match source inventory. Only
   after those pass should the tracker row and revision be marked complete.
