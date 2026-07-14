# Item 82 research: reject complete invalid ACP JSON-RPC envelopes

## Verdict

- **Priority:** P2
- **Root-cause confidence:** High
- **Fix confidence:** High
- **Owning layer:** `agentos-sidecar-core`, shared by native and browser
- **Client change:** none
- **Recommendation:** fix the silent ignore, but do **not** restore the legacy
  harness's JSON-RPC `-32600` wire reply for the response-shaped fixture.

This is a real production bug, but the obsolete test's expected remedy is not the
right production contract. The exact legacy fixture is:

```json
{"jsonrpc":"2.0","result":{"ok":true}}
```

It is complete and syntactically valid JSON, but it is not valid JSON-RPC. It has
`result`, so it is response-shaped, and JSON-RPC 2.0 requires every Response object
to contain `id`. It is not a Notification because notifications require a string
`method`. ACP is bidirectional JSON-RPC over stdio, and stdout is reserved for ACP,
so the sidecar should fail the active interaction immediately with a typed protocol
error instead of treating this as harmless stdout.

The legacy `-32600` behavior should not be restored. `-32600` is a server Response
to an invalid **Request**. The fixture above is a malformed **Response** from the
adapter while the sidecar is waiting for a correlated response. Sending a Response
to that malformed Response is not a valid correlation, creates uncorrelated wire
traffic, and still does not complete the original interaction. The safe behavior is
a local sidecar error plus the existing adapter-abort cleanup.

Primary protocol references:

- [ACP architecture](https://agentclientprotocol.com/get-started/architecture)
  describes ACP as bidirectional JSON-RPC over stdin/stdout.
- [JSON-RPC 2.0, Response object](https://www.jsonrpc.org/specification#response_object)
  requires `id` and exactly one of `result` or `error`.
- [JSON-RPC 2.0, errors](https://www.jsonrpc.org/specification#error_object)
  defines `-32600` as “Invalid Request,” not as a response to an invalid Response.

## Why this matters

The current behavior converts a precise protocol violation into a misleading
timeout:

- initialize can wait 10 seconds;
- `session/new` can wait 30 seconds;
- ordinary methods can wait 120 seconds;
- `session/prompt` can wait 600 seconds.

All waits remain bounded, so this is not a P0/P1 security-boundary escape. It is a
P2 correctness, availability, and diagnostics issue: a broken adapter can retain a
process/interaction until the deadline and the caller receives “timeout” instead of
“response is missing id.” A malicious adapter could already remain silent for the
same bounded duration, so the change does not close a distinct privilege bypass.

## Exact production failure chain

### Shared classification

`crates/agentos-sidecar-core/src/behavior.rs` owns
`classify_json_rpc_message`. It classifies only by the presence of `id` and a
string-valued `method`:

```rust
match (
    message.get("id").is_some(),
    message.get("method").and_then(Value::as_str).is_some(),
) {
    (true, true) => AcpJsonRpcMessageKind::InboundRequest,
    (true, false) => AcpJsonRpcMessageKind::Response,
    (false, true) => AcpJsonRpcMessageKind::Notification,
    (false, false) => AcpJsonRpcMessageKind::Unknown,
}
```

The legacy missing-id response reaches `(false, false)` and becomes `Unknown`.
The existing production unit test
`behavior::tests::json_rpc_classifier_prioritizes_inbound_requests_over_notifications`
explicitly locks in that classification for `{"jsonrpc":"2.0"}`.

### Native production route

Native does not use the legacy `native-sidecar/src/json_rpc.rs` codec in its real
extension route. That route is:

1. `crates/agentos-sidecar/src/acp_extension.rs::AcpExtension::run_core_transition`
   calls `AcpCore::dispatch_resumable`.
2. `AcpExtension::drive_native_pending` drains the adapter stdout and sends an
   `AcpDeliverAgentOutputRequest` back through the core.
3. `crates/agentos-sidecar-core/src/engine.rs` parses complete lines with
   `AcpJsonLineAccumulator`, then calls `classify_json_rpc_message` in all four
   resumable state machines:
   - `advance_create`
   - `advance_resume`
   - `advance_restart`
   - `advance_prompt`
4. Each match currently groups
   `AcpJsonRpcMessageKind::Response | AcpJsonRpcMessageKind::Unknown => {}`.
5. The subsequent response-id check cannot match because `id` is absent, so the
   interaction is reinserted as pending. Native keeps polling until the applicable
   timeout.

The browser wrapper in `crates/agentos-sidecar-browser/src/lib.rs` also calls
`AcpCore::dispatch_resumable`, so the same shared fix supplies browser parity.

### Blocking core route

`crates/agentos-sidecar-core/src/json_rpc.rs::send_json_rpc_exchange` also ignores
`Unknown` alongside unmatched Responses. Production native currently drives the
resumable route, but this blocking route remains exercised by core conformance and
must not retain divergent semantics.

### Why the legacy test is not production evidence

`crates/native-sidecar/tests/acp_session.rs::malformed_acp_frames_with_missing_ids_return_invalid_request_errors`
uses `tests/acp_legacy/client.rs`, which in turn uses the dead typed codec from
`crates/native-sidecar/src/json_rpc.rs`. That test-only client responds to every
deserialization failure with the codec's `JsonRpcParseError::to_response()`.
Production does not call that codec or read loop. Item 81 can delete those files;
Item 82 should not preserve them merely to keep the old error shape.

The codec is still present at the time of this research, but `rg` finds no
production caller outside its own module; only the legacy test modules import it.
“Dead” here means disconnected from the real ACP extension route, not already
deleted from the tree.

## Exact code map

Line numbers below describe the Item 80 working tree and are navigation hints;
symbols are the stable edit targets.

| File / current line context | Symbol | Exact Item 82 edit |
|---|---|---|
| `crates/agentos-sidecar-core/src/behavior.rs:24-47` | `AcpJsonRpcMessageKind`, `classify_json_rpc_message` | Make classification fallible. Return `InvalidState` for a complete object with `result` or `error` but no `id`; use the focused `response is missing id` diagnostic. Reject the remaining `Unknown` envelope shapes with the generic diagnostic rather than silently accepting non-protocol stdout. |
| `crates/agentos-sidecar-core/src/behavior.rs:927-949` | `json_rpc_classifier_prioritizes_inbound_requests_over_notifications` | Keep the three valid-classification assertions and add the exact missing-id response fixture plus generic invalid-envelope cases. |
| `crates/agentos-sidecar-core/src/json_rpc.rs:86-130` | `send_json_rpc_exchange` | Apply `?` to the fallible classification before routing. Keep a valid Response with a different `id` ignored; remove only the `Unknown` ignore path. Do not call `write_json_line` for the classification error. |
| `crates/agentos-sidecar-core/src/json_rpc.rs:157-309` | `EchoHost` and exchange unit tests | Add a deterministic missing-id fixture and assert `invalid_state` plus exactly one host write (the original Request). |
| `crates/agentos-sidecar-core/src/engine.rs:1663-1720` | `advance_create` | Propagate the classifier error. `feed_create` has already removed pending state and wraps the failure with `with_abort_cleanup`. |
| `crates/agentos-sidecar-core/src/engine.rs:1809-1836` | `advance_resume` | Propagate the same error before response-id matching; retain existing notification/inbound-request handling. |
| `crates/agentos-sidecar-core/src/engine.rs:2172-2309` | `advance_restart` | Propagate the same error. Preserve restart-specific session removal and retained-cleanup behavior in `feed_restart`. |
| `crates/agentos-sidecar-core/src/engine.rs:2312-2389` | `advance_prompt` | Propagate the same error. Preserve prompt preamble restoration and session cleanup behavior in `feed_prompt`. |
| `crates/agentos-sidecar-core/src/engine.rs:6100-6114` | resumable terminal-error tests | Add the missing-id regression beside the existing parse-error cleanup test; assert pending/session counts and the one `SIGKILL`. |
| `crates/agentos-sidecar/tests/acp_extension.rs:31-39` | `acp_extension_suite` | Register the native E2E regression inside this serial suite. Do not add a separately scheduled native-sidecar test. |
| `crates/agentos-sidecar/tests/acp_extension.rs:1100-1197, 1509-1716` | `dispatch_acp*`, VM/package setup helpers | Reuse the real extension dispatch and explicit guest mount/package setup. Add only the malformed adapter fixture and lifecycle assertions. |
| `crates/agentos-sidecar/src/acp_extension.rs:417-500` and `crates/agentos-sidecar-browser/src/lib.rs:195-206` | native/browser core dispatch wrappers | No production edit expected: both already turn the shared `AcpCoreError` into `AcpErrorResponse`. Inspect in review to prevent an unnecessary client/backend parser. |

## Recommended production edit

Make invalid-envelope rejection a single shared-core decision. Do not add another
native-only parser, do not copy the legacy typed codec, and do not add behavior to
the TypeScript or Rust SDK.

### 1. Make the shared classifier fallible

Edit `crates/agentos-sidecar-core/src/behavior.rs`:

1. Remove `AcpJsonRpcMessageKind::Unknown`.
2. Change `classify_json_rpc_message` to return
   `Result<AcpJsonRpcMessageKind, AcpCoreError>`.
3. Preserve the current priority for request/response/notification messages.
4. Return `AcpCoreError::InvalidState` for `(false, false)`.
5. Use a focused message without echoing the potentially large adapter payload.

Concrete target shape:

```rust
pub fn classify_json_rpc_message(
    message: &Value,
) -> Result<AcpJsonRpcMessageKind, AcpCoreError> {
    match (
        message.get("id").is_some(),
        message.get("method").and_then(Value::as_str).is_some(),
    ) {
        (true, true) => Ok(AcpJsonRpcMessageKind::InboundRequest),
        (true, false) => Ok(AcpJsonRpcMessageKind::Response),
        (false, true) => Ok(AcpJsonRpcMessageKind::Notification),
        (false, false) if message.get("result").is_some() || message.get("error").is_some() => {
            Err(AcpCoreError::InvalidState(String::from(
                "ACP adapter emitted invalid JSON-RPC response: response is missing id",
            )))
        }
        (false, false) => Err(AcpCoreError::InvalidState(String::from(
            "ACP adapter emitted invalid JSON-RPC envelope: message has neither a string method nor an id",
        ))),
    }
}
```

This deliberately fixes the discovered category without recreating full schema
validation. Do not broaden Item 82 into checking every `jsonrpc`, `params`, `id`,
and result/error exclusivity rule; the official upstream adapter SDKs own their
wire types, and a copied full parser would recreate the Item 81 problem. A later
strict-envelope item can add more centralized checks if real adapter failures show
they are needed.

### 2. Propagate the shared error from every exchange

Edit `crates/agentos-sidecar-core/src/json_rpc.rs`:

- Change `match classify_json_rpc_message(&message)` to
  `match classify_json_rpc_message(&message)?`.
- Remove the `Unknown` match arm.
- Keep unmatched, otherwise valid Responses ignored; a delayed response for a
  different request id is a separate correlation case.

Edit all four classifier sites in
`crates/agentos-sidecar-core/src/engine.rs` (`advance_create`, `advance_resume`,
`advance_restart`, and `advance_prompt`) the same way:

- use `classify_json_rpc_message(&message)?`;
- remove `Unknown` from the Response arm;
- retain all current inbound-request, notification-capacity, response-id, and
  cleanup logic.

The surrounding `feed_create`, `feed_resume`, `feed_restart`, and `feed_prompt`
methods already remove pending state first and call `with_abort_cleanup` on a
terminal error. No new kill/cleanup state machine is needed.

### 3. Do not send `-32600` for the missing-id response

There should be no new call to `write_json_line` for this error. The adapter wrote
an invalid Response, so the extension should return the core's existing sidecar
error shape:

```text
code: invalid_state
message: ACP adapter emitted invalid JSON-RPC response: response is missing id
```

Native `AcpExtension::dispatch_shared_core` and the browser extension already
convert an `AcpCoreError` into `AcpErrorResponse`; no wrapper edit is required.

If a future item wants strict handling for an unmistakably invalid inbound Request
(for example an object with no `result`/`error` and a non-string `method`), it may
centralize an `InvalidRequest` classification and send `-32600` with `id: null`.
That is distinct from replying to the response-shaped fixture tracked here.

## Tests

### Before behavior evidence

- [x] First add a characterization assertion on the item's parent to
  `json_rpc_classifier_prioritizes_inbound_requests_over_notifications` for the
  exact fixture `{"jsonrpc":"2.0","result":{"ok":true}}`. Run
  `cargo test -p agentos-sidecar-core --lib json_rpc_classifier_prioritizes_inbound_requests_over_notifications`.
  It currently returns `Unknown`, proving the shared production classifier does
  not recognize the malformed Response.
- [x] Add a temporary resumable characterization beside the existing terminal
  parse-error test: begin a resume, feed the exact missing-id fixture, and assert
  `ResumeStep::Pending`, pending resume count `1`, and no recorded kill. This is a
  deterministic proof that the production state machine keeps waiting; it avoids
  sleeping for the 10-second initialize deadline. Run it on the parent, record the
  result in the tracker, then rewrite that same test into the after regression.
- Optional blocking-loop timeout characterization was not needed; the lasting
  no-wire-reply regression instead gives `EchoHost` a missing-id injection and
  proves immediate failure deterministically. The original optional approach would
  have given `EchoHost` a switch that
  suppresses its automatic matching reply, injecting only the missing-id object,
  and using the mock clock to assert the current `execution` timeout. This exercises
  the one non-resumable ignore arm without a wall-clock wait.
- [x] Before Item 81 deletes the harness (run against Item 80 or Item 81's parent), run
  `cargo test -p agentos-native-sidecar --test acp_session malformed_acp_frames_with_missing_ids_return_invalid_request_errors`.
  This proves only that the obsolete harness emitted `-32600`; record it as the
  behavior being consciously replaced, not as the target production contract.
- [x] Inspect/record the five current `Unknown` arms with
  `rg -n 'AcpJsonRpcMessageKind::.*Unknown|Unknown =>' crates/agentos-sidecar-core/src`.
  There should be one blocking and four resumable ignore sites before the fix.

### After unit coverage

In `crates/agentos-sidecar-core/src/behavior.rs`, replace/rename the classifier test
with `json_rpc_classifier_rejects_complete_invalid_envelopes`:

- valid inbound Request, Response, and Notification cases still return their
  existing kinds;
- `{"jsonrpc":"2.0","result":{"ok":true}}` returns `invalid_state` with
  `response is missing id`;
- `{"jsonrpc":"2.0"}` and a non-object complete JSON value return
  `invalid_state` with the generic invalid-envelope message.

In `crates/agentos-sidecar-core/src/json_rpc.rs`, add
`complete_response_without_id_fails_without_a_wire_reply` using `EchoHost`'s
existing `inbound_before_response` injection:

1. inject `{"jsonrpc":"2.0","result":{"ok":true}}` before the normal matching
   response;
2. call `send_json_rpc_exchange` with a large timeout;
3. assert it returns `invalid_state` immediately;
4. assert `host.writes.len() == 1`, proving the sidecar wrote only its original
   request and did not send the legacy `-32600` response.

In `crates/agentos-sidecar-core/src/engine.rs`, add
`resumable_resume_missing_response_id_clears_state_and_aborts_agent` beside
`resumable_resume_terminal_parse_error_clears_state_and_kills_agent`:

1. begin a resumable resume;
2. feed
   `b"{\"jsonrpc\":\"2.0\",\"result\":{\"protocolVersion\":1}}\n"` while
   awaiting initialize;
3. assert `invalid_state` and `response is missing id`;
4. assert pending resume count and session count are zero;
5. assert the host recorded one `SIGKILL` for the adapter.

This test covers the exact production state machine used by native and browser,
including cleanup, rather than only testing the classifier.

### After native end-to-end coverage

Add a scenario function named
`acp_missing_initialize_response_id_fails_closed` to
`crates/agentos-sidecar/tests/acp_extension.rs` and call it from
`acp_extension_suite`, reusing the existing sidecar, VM, package, and dispatch
helpers. Keeping it inside that suite preserves this file's intentional serial
native-runtime execution. Its small adapter should reply to `initialize` with a
`result` but omit `id`:

```js
process.stdout.write(`${JSON.stringify({
  jsonrpc: "2.0",
  result: { protocolVersion: message.params.protocolVersion }
})}\n`);
```

After Item 80, use `/workspace` as the ACP request cwd and expose the host fixture
only through the test's explicit `host_dir` mount/package configuration. Do not
reintroduce a raw host cwd or entrypoint just for this regression.

Assert:

1. `AcpCreateSessionRequest` returns `AcpErrorResponse` with code
   `invalid_state` and message `response is missing id`;
2. `AcpListSessionsRequest` returns no ACP session, proving bootstrap did not
   partially commit;
3. closing the owning wire session succeeds (and therefore disposes its VM),
   proving the native process route was removed and lifecycle cleanup completed.

Do not assert a wall-clock threshold in this integration test. Immediate failure is
proved deterministically by the core `EchoHost` test; a sub-second wall-clock bound
would be flaky in CI.

### Validation commands after the fix

- [x] `cargo test -p agentos-sidecar-core --lib json_rpc_classifier_rejects_complete_invalid_envelopes`
- [x] `cargo test -p agentos-sidecar-core --lib complete_response_without_id_fails_without_a_wire_reply`
- [x] `cargo test -p agentos-sidecar-core --lib resumable_resume_missing_response_id_clears_state_and_aborts_agent`
- [x] `cargo test -p agentos-sidecar --test acp_extension acp_extension_suite -- --nocapture`
- [x] `cargo test -p agentos-sidecar-core --lib` (80/80)
- [x] `cargo test -p agentos-sidecar-core --test acp_conformance` (8/8)
- [x] `cargo test -p agentos-sidecar --test acp_wrapper_conformance` (15/15)
- [x] `cargo check --workspace`
- [x] `cargo fmt --all --check`
- [x] `git diff --check`
- [x] `rg -n 'AcpJsonRpcMessageKind::Unknown|Unknown =>' crates/agentos-sidecar-core/src`
  returns no hit.

## Implementation result

Revision `vsqvzlkn` makes complete invalid ACP envelopes terminal in the shared
sidecar core. A response without `id` returns focused `invalid_state`, no
`-32600` is written back to the adapter, and the same classifier feeds blocking,
native resumable, and browser resumable paths. The native abort host also confirms
the killed adapter's exit before releasing its route, so immediate owner teardown
cannot race a still-active execution. No SDK code changed.

## Scope and dependencies

1. Item 81 may delete the legacy codec/harness before or after this fix. Preserve
   the before-test result in the tracker; do not retain dead files for Item 82.
2. Item 82 is already in the sidecar-owned shared core. Nothing should move into
   either client, and no client default or callback is required.
3. Do not add a sidecar policy knob for tolerating JSON-shaped stdout. ACP reserves
   adapter stdout for protocol traffic; diagnostics belong on stderr.
4. Do not change timeout defaults. The fix makes an invalid complete envelope
   terminal before those existing deadlines.
5. Do not turn unmatched but structurally valid response ids into this error. Late
   response correlation and tombstones are a separate behavior decision.

## Cleanup implications and risks

| Risk / implication | Required handling |
|---|---|
| Classification becomes terminal before the pending state is reinserted. | Preserve the current remove-first `feed_*` wrappers. Their `with_abort_cleanup` call must remain the sole fail-closed process cleanup path. |
| Native abort can itself fail. | Preserve existing `cleanup_failed` aggregation and retry state; do not hide a kill/route-cleanup failure in order to preserve the primary `invalid_state` code. The focused normal-path test should expect `invalid_state`; existing cleanup-failure tests remain authoritative for the exceptional path. |
| Removing `Unknown` also rejects `{}`, `null`, and other complete JSON values previously ignored. | This is intentional because ACP owns adapter stdout. Keep the change limited to classification of complete parsed values; partial lines and whitespace retain `AcpJsonLineAccumulator` behavior. |
| A valid but late Response has an `id` different from the active request. | Continue ignoring it in this item. Treating late ids as fatal requires a separate correlation/tombstone design. |
| Replying with legacy `-32600` could create uncorrelated protocol traffic. | Assert the blocking host records only the original outbound Request. The caller-facing failure is `AcpErrorResponse { code: "invalid_state", ... }`, not another JSON-RPC frame to the adapter. |
| Native E2E tests share heavyweight runtime state. | Add the scenario to `acp_extension_suite` so it runs serially with the existing scenarios, and avoid timing assertions. |
| Item 81 removes the only old test expecting `-32600`. | Record its before result in the tracking document before deletion; Item 82 must not recreate the legacy parser merely to preserve that obsolete expectation. |

## Proposed diff sequence

1. On Item 82's parent, add/run the exact missing-id characterization in the
   classifier and resumable engine, and record the observed `Unknown`/`Pending`
   behavior in `docs/thin-client-migration.md` before changing the expectation.
2. In `behavior.rs`, make `classify_json_rpc_message` return `Result`, remove
   `Unknown`, and add the two bounded diagnostics without embedding adapter output.
3. In `json_rpc.rs` and the four `engine.rs` state machines, propagate the
   classifier error and leave all valid message routing/correlation logic intact.
4. Rewrite the characterization into after tests: classifier rejection, blocking
   no-wire-reply, and resumable abort/cleared-state coverage.
5. Add the malformed-initialize scenario to the serial native
   `acp_extension_suite`, using only guest paths plus explicit test mounts, and
   assert error taxonomy, no partial ACP session, and successful owner teardown.
6. Run the focused and conformance commands above, update Item 82's three tracking
   checkboxes, format/check the tree, and seal the work in Item 82's dedicated
   stacked `jj` revision.

## Proposed completion statement

Item 82 is complete when shared ACP core rejects a complete response without `id`
as an immediate typed `invalid_state`, native/browser resumable cleanup is covered,
the blocking core path has identical semantics, and no client or legacy JSON-RPC
parser has been added.
