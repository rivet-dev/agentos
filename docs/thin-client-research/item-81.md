# Item 81 research: remove the test-only legacy ACP state machine

## Original issue

The native-sidecar test tree still builds a retired ACP client as if it were a
second product implementation. `acp_legacy::client::AcpClient` owns request IDs,
timeouts, JSON-line framing, pending response senders, permission request retention,
deduplication, cancellation fallback, and terminal failure state;
`acp_legacy::session::AcpSessionState` separately owns bootstrap and session-update
state. The shipped ACP path owns those behaviors in `agentos-sidecar-core` and the
native ACP extension instead. Keeping the copy makes a green native test capable of
proving only the obsolete harness, not the runtime users execute.

This code should be **deleted, not moved into the sidecar**. The real implementation
is already sidecar-owned. Only unique behavioral assertions should move to tests of
that implementation.

## Recommendation

Delete the legacy harness rather than moving it. It compiles a complete second ACP
client/session implementation into tests, including permission request retention,
deduplication, legacy `request/permission` notifications, timeout state, JSON-RPC
framing, and synthetic session state. None of that code is used by the shipped
native sidecar.

- **Priority:** P3 cleanup. It is test-only/dead code, so it does not change runtime
  behavior today, but it can falsely validate behavior that production no longer
  has.
- **Fix confidence:** High.
- **Recommended implementation confidence:** High after the three small coverage
  gaps below are moved to production-path tests.

The cleanup is larger than the two files named in the tracker. The directory is
2,479 lines, and its inline tests are compiled into three integration-test binaries.
The full legacy surface accounts for 43 unique tests and 61 test executions:

- `acp_integration`: 23 executions (nine inline legacy tests plus fourteen tests in
  `tests/acp/`)
- `acp_session`: 29 executions (the same nine inline tests plus twenty session tests)
- `service`: the same nine inline legacy tests again, even though the service tests
  never use the legacy ACP API

## Exact code to remove

Delete:

- `crates/native-sidecar/tests/acp_legacy/mod.rs`
- `crates/native-sidecar/tests/acp_legacy/client.rs`
- `crates/native-sidecar/tests/acp_legacy/compat.rs`
- `crates/native-sidecar/tests/acp_legacy/session.rs`
- `crates/native-sidecar/tests/acp_legacy/timeout.rs`
- `crates/native-sidecar/tests/acp_integration.rs`
- `crates/native-sidecar/tests/acp/mod.rs`
- `crates/native-sidecar/tests/acp/client.rs`
- `crates/native-sidecar/tests/acp/json_rpc.rs`
- `crates/native-sidecar/tests/acp_session.rs`
- `crates/native-sidecar/src/json_rpc.rs`

The last file is the dead codec companion used only by the legacy tests. Production
declares it as `#[allow(dead_code)] pub(crate) mod json_rpc`, and repository-wide
search finds no production caller.

Remove these declarations:

- `crates/native-sidecar/src/lib.rs`: remove the dead-code `json_rpc` module.
- `crates/native-sidecar/tests/service.rs`: remove both path modules at the top:
  `#[path = "acp_legacy/mod.rs"] mod acp` and
  `#[path = "../src/json_rpc.rs"] mod json_rpc`. No service test refers to either.
- `crates/native-sidecar/CLAUDE.md`: remove the `Local Patterns` bullet that calls
  `tests/acp_legacy/` a fixture. The directory no longer exists after this item, and
  the following clause is already enforced by the surrounding extension-ownership
  rules.

Do not replace these files with another native-only fixture layer. Retained tests
should exercise `agentos-sidecar-core` or the real native/browser wrappers.

### Symbol and line-context deletion map

Use symbol anchors rather than relying only on line numbers, since the Item 80 stack
can shift the service test file while Item 81 is waiting:

| Location | Current anchor/context | Exact edit |
|---|---|---|
| `crates/native-sidecar/src/lib.rs:12-13` | `#[allow(dead_code)] pub(crate) mod json_rpc;` | Delete the attribute and module declaration. Repository search shows no production caller. |
| `crates/native-sidecar/tests/service.rs:3-7` | `#[path = "acp_legacy/mod.rs"] mod acp;` | Delete the full path-module declaration and its allow attribute. No service test refers to `crate::acp`; only the module's nine inline tests make it appear used. |
| `crates/native-sidecar/tests/service.rs:24-27` | `#[path = "../src/json_rpc.rs"] mod json_rpc;` | Delete the full path-module declaration and its allow attribute. It exists only to satisfy the legacy module's imports. |
| `crates/native-sidecar/tests/acp_integration.rs:1-9` | path modules `acp_legacy`, `json_rpc`, and `acp/mod.rs` | Delete this integration-test root after its retained coverage is mapped. It contains no production-path setup of its own. |
| `crates/native-sidecar/tests/acp_session.rs:1-33` | path modules plus imports from `acp::compat` and `acp::session` | Delete the entire 1,128-line integration test. Every assertion instantiates the retired structs or codec. |
| `crates/native-sidecar/tests/acp_legacy/client.rs:22-1100+` | `AcpClient`, `AcpClientInner`, `read_loop`, `handle_inbound_request`, retention helpers | Delete the file; do not transplant its `pending`, `seen_inbound_request_ids`, `pending_permission_requests`, recent-activity, or timeout state. |
| `crates/native-sidecar/tests/acp_legacy/compat.rs:14-476` | `AgentCompatibilityKind`, `PendingPermissionRequests`, `SeenInboundRequestIds`, permission normalization and synthetic update helpers | Delete the file. The only retained semantic is permission option alias selection, tested at production `permission_result`. |
| `crates/native-sidecar/tests/acp_legacy/session.rs:12-506` | `AcpSessionState`, `validate_initialize_result`, synthetic mode/config state, terminal buffer | Delete the file. Test protocol mismatch and arbitrary JSON config values through shared core instead. |
| `crates/native-sidecar/tests/acp_legacy/timeout.rs:7-72` | `AcpTimeoutDiagnostics` | Delete the file. Production timeout ownership and cleanup are already in shared core/extension tests. |
| `crates/native-sidecar/tests/acp_legacy/mod.rs:1-15` | legacy re-exports | Delete the file and then the empty directory. |
| `crates/native-sidecar/tests/acp/client.rs:60-630` | nine `client_*` tests | Delete with `acp_integration.rs`; each drives `acp_legacy::AcpClient`, not the sidecar. |
| `crates/native-sidecar/tests/acp/json_rpc.rs:9-121` | five `json_rpc_*` tests | Delete with `acp_integration.rs`; each drives the unused native typed codec. |
| `crates/native-sidecar/tests/acp/mod.rs:1-2` | `mod client; mod json_rpc;` | Delete the file and then the empty directory. |
| `crates/native-sidecar/src/json_rpc.rs:11-448` | `JsonRpcId` through `parsed_id` | Delete the whole private codec after all three path-module imports above are gone. Do not confuse it with `agentos-sidecar-core/src/json_rpc.rs`, which remains authoritative and in use. |
| `crates/native-sidecar/CLAUDE.md:22` | `Legacy ACP helpers under tests/acp_legacy/ are fixtures only` | Delete this stale local-pattern bullet; do not replace it with a harness-specific rule. |

The production assertions to add are anchored at:

- `crates/agentos-sidecar/src/acp_extension.rs:2861` (`permission_result`) and the
  nearby test `missing_client_permission_reply_uses_sidecar_default` around line
  3152.
- `crates/agentos-sidecar-core/src/engine.rs:4577`
  (`validate_initialize_result`) and the nearby resumable-create test
  `resumable_create_session_propagates_an_agent_initialize_error` around line 5966.
- `crates/agentos-sidecar-core/src/behavior.rs:1056`
  (`successful_config_updates_state_and_rejects_malformed_options`).

## Permission-state inventory

All permission-retention assertions are obsolete and should be deleted, not ported.
The old design converted `session/request_permission` into a legacy
`request/permission` notification, retained pending requests and seen JSON-RPC IDs,
then accepted a later client RPC to complete the adapter response. Production now
handles the inbound request inside the native ACP extension and waits on one bounded
host callback; Item 28 already proves sidecar-owned deadline/default/cleanup behavior.
There is no pending-ID compatibility table to preserve.

| Legacy assertion | Disposition | Authoritative coverage/reason |
|---|---|---|
| `client_shims_modern_permission_requests_to_legacy_notifications` | Delete | The `request/permission` shim is not a current protocol contract. `acp_extension_suite` exercises the real `AcpPermissionCallback` and adapter response. |
| `client_normalizes_opencode_style_permission_option_ids` | Port only the option-resolution table | Production `permission_result`/`resolve_permission_option_id` still intentionally accepts `once`/`always`/`reject` and `allow_once`/`allow_always`/`reject_once`; current integration covers only the `once` path. |
| `client_deduplicates_repeated_permission_request_ids` and `permission_requests_are_normalized_and_deduped` | Delete | Production replies to each inbound request directly and retains no seen-ID cache. Reintroducing dedupe would create a second state machine. |
| `client_seen_request_ids_stay_bounded_after_many_unique_requests` and `seen_inbound_request_ids_evict_oldest_entry_after_retention_window` | Delete | The production sidecar has no `SeenInboundRequestIds`. Core process/event bounds and the single permission wait per owner/session/process are authoritative. |
| `client_pending_permission_requests_stay_bounded_with_seen_request_ids`, `session_pending_permission_requests_are_bounded_independently`, and `permission_requests_evict_pending_entries_with_seen_request_window` | Delete | The production path has no pending permission collection. Item 28 tests the live callback route's deadline and cleanup. |
| `client_permission_reply_survives_unrelated_seen_request_id_eviction` and `session_permission_reply_survives_unrelated_seen_request_id_eviction` | Delete | There is no unrelated seen-ID eviction in the production callback path. |
| `pending_permission_eviction_uses_typed_json_rpc_ids` | Delete | No retained JSON-RPC-ID map exists in production. |
| `client_permission_ids_are_collision_safe_for_string_and_number_ids` and `permission_ids_are_collision_safe_for_string_and_number_ids` | Delete | Production preserves the inbound JSON-RPC `id` on the immediate response; it does not stringify IDs into a map key. The native integration already correlates numeric ID `99`, and core/browser inbound tests preserve string ID `"host-1"`. |

### Permission coverage to add before deletion

In `crates/agentos-sidecar/src/acp_extension.rs`, add a table-driven unit test next
to `missing_client_permission_reply_uses_sidecar_default`:

`permission_results_preserve_adapter_option_ids_for_all_reply_aliases`

It should call the production `permission_result` with options whose IDs are
`once`, `always`, and `reject`, then verify both short and canonical replies select
the adapter-provided ID:

- `once` and `allow_once` -> `once`
- `always` and `allow_always` -> `always`
- `reject` and `reject_once` -> `reject`
- an unknown reply -> `{ "outcome": { "outcome": "cancelled" } }`

Keep the existing Item 28 tests as the authority for omitted reply and typed timeout
defaults. Do not copy legacy pending/seen state into `agentos-sidecar-core`.

## Remaining legacy tests mapped to authoritative code

### `tests/acp/client.rs`

| Test | Disposition or current authority |
|---|---|
| `client_correlates_responses_and_forwards_notifications` | Covered by core `json_rpc::tests::round_trips_a_json_rpc_request_against_a_mock_agent`, core conformance `create_bootstrap_and_prompt_notification_text_are_strategy_identical`, and wrapper lifecycle conformance. |
| Three permission tests | See permission inventory above. |
| `client_falls_back_to_cancel_notification_when_request_form_is_unsupported` | Covered by behavior `wire_notifications_and_cancel_fallback_are_strict` and conformance `unsupported_cancel_uses_the_same_notification_fallback`. |
| `client_timeout_errors_include_recent_activity` | Delete. The legacy recent-activity ring and process-state provider do not exist in production. Current typed timeouts are covered by core JSON-RPC timeout tests, `acp_request_timeout`, adapter stderr surfacing, and wrapper abort/cleanup tests. |
| `client_rejects_adapter_lines_over_configured_limit` | Covered by behavior `bounded_json_lines_preserve_partials_and_reject_bad_input`, which exercises the production accumulator and exact boundary. |
| `client_waits_for_exit_drain_before_rejecting_pending_requests` | Delete the fixed 50 ms grace behavior. Current lifecycle authority is core/wrapper cleanup and adapter restart/stderr coverage, not a quiet timer in a client. |
| `client_handles_inbound_requests_with_registered_handler` | Covered by core `json_rpc::tests::inbound_request_is_answered_without_consuming_notification_capacity`, engine `resumable_inbound_request_stays_pending_and_bypasses_event_capacity`, native `acp_terminal_requests_stay_inside_sidecar`, and browser `browser_wrapper_rejects_inbound_host_requests_during_create`. |

### `tests/acp/json_rpc.rs`

The five tests (`json_rpc_codec_round_trips_all_message_shapes`, both deserializer
rejection tests, optional error data, and result/error exclusivity) test only the
unused typed codec in `native-sidecar/src/json_rpc.rs`. Production intentionally uses
`serde_json::Value`, `AcpJsonLineAccumulator`, and
`classify_json_rpc_message`; current coverage is:

- behavior `bounded_json_lines_preserve_partials_and_reject_bad_input`
- behavior `json_rpc_classifier_prioritizes_inbound_requests_over_notifications`
- behavior `unsupported_inbound_request_response_preserves_request_identity`
- core JSON-RPC round-trip/inbound/timeout tests
- generated protocol `codec::tests::request_round_trips_through_bare`

Delete the old typed-codec assertions. Do not recreate a second parser just to retain
its error types.

One related production behavior deserves a separate explicit decision: a complete,
valid JSON value with neither `id` nor `method` is currently classified `Unknown` and
ignored by the blocking and resumable loops. The legacy
`malformed_acp_frames_with_missing_ids_return_invalid_request_errors` test expected a
JSON-RPC `-32600` reply instead. This is not behavior supplied by the shipped codec,
so it should not block Item 81 deletion; file a focused follow-up if strict unknown-
frame rejection is desired. The safe recommendation is a shared core typed error,
not restoration of the legacy parser.

### `acp_session.rs`: bootstrap and state

| Legacy test(s) | Disposition or current authority |
|---|---|
| `session_state_tracks_metadata_and_derived_model_option` | Covered by behavior `bootstrap_derives_models_and_honors_session_overrides`, core session response methods, core conformance create, and native/browser wrapper lifecycle. |
| `initialize_request_uses_requested_protocol_version_and_client_capabilities` | Covered through actual create writes in core engine and shared wrapper/conformance fixtures. |
| `initialize_result_accepts_matching_protocol_version` | Covered by every successful core create/resume fixture. |
| `initialize_result_reports_protocol_version_mismatch` | **True gap:** production has the check, but no focused production-path assertion. Add the engine test described below. |
| `session_state_does_not_duplicate_existing_model_options` | Covered by behavior `bootstrap_derives_models_and_honors_session_overrides`; strengthen that test with an explicit count of one model option if desired. |
| `notifications_update_session_snapshot_without_retaining_replay_events` | Covered by behavior state-update tests, core conformance writable config/mode tests, and bounded/acknowledged sidecar event tests. |
| `acp_stdout_buffer_trimming_keeps_newest_utf8_boundary` | Delete. Production rejects an over-limit line with a typed error rather than silently trimming a replay buffer; `bounded_json_lines_preserve_partials_and_reject_bad_input` is authoritative. |

Add to `crates/agentos-sidecar-core/src/engine.rs` beside
`resumable_create_session_propagates_an_agent_initialize_error`:

`resumable_create_session_rejects_protocol_version_mismatch_and_cleans_up`

Begin a real resumable create, feed an initialize response reporting version `2`
for a version-`1` request, assert the production error contains both requested and
reported versions, and assert pending create/process route state is cleaned up. This
tests the shipped helper instead of the deleted copy.

### `acp_session.rs`: mode and config state

| Legacy test(s) | Disposition or current authority |
|---|---|
| Both mode synthetic/no-duplicate tests | Covered by behavior `successful_mode_updates_state_and_synthesizes_only_when_needed` and conformance `mode_success_updates_authoritative_state_in_both_strategies`. |
| Both config synthetic/no-duplicate tests | Covered by behavior `successful_config_updates_state_and_rejects_malformed_options` and conformance `writable_and_read_only_config_update_authoritative_state_in_both_strategies`. |
| `config_changes_accept_non_string_values` | **True narrow gap:** production supports arbitrary JSON values, but its unit test uses only strings. Extend the behavior test with a boolean and assert the stored/synthetic `currentValue` remains boolean. |
| `config_changes_return_typed_error_for_malformed_option_entries` | Covered by behavior `successful_config_updates_state_and_rejects_malformed_options`. |
| `config_changes_return_typed_error_for_malformed_params` | Delete the old error-shape assertion. Current generic request behavior ignores a non-string `configId`, while the generated `AcpSetSessionConfigRequest` carries strings. If strict generic-param validation is desired, track it as a production-core follow-up rather than preserving a test-copy-only error enum. |

### `acp_session.rs`: transport, timeout, and cancellation

| Legacy test(s) | Disposition or current authority |
|---|---|
| `cancel_method_not_found_detects_session_cancel_response_shape` | Covered by behavior `wire_notifications_and_cancel_fallback_are_strict` and shared conformance. |
| Both inbound-handler wait/timeout tests | Delete the async client-handler timing model. Core owns synchronous/resumable host handling; current native/browser tests cover supported and unsupported host requests. |
| `malformed_acp_frames_with_missing_ids_return_invalid_request_errors` | See the explicit unknown-frame follow-up above. Do not preserve the dead codec solely for this assertion. |
| `acp_response_write_failures_put_the_client_into_a_failed_state` | Covered at the current host boundary by core error propagation/abort cleanup and wrapper initial/prompt failure cleanup. The legacy permanent client failure cache no longer exists. |
| `acp_request_method_timeout_overrides_apply_to_initialize_and_prompt` | Covered by engine pending-response timeout assertions (10 s initialize, 30 s new, 600 s prompt) and `acp_request_timeout` (600 s prompt versus 120 s control). Runtime callers no longer supply a client timeout map. |
| `acp_timed_out_session_prompt_sends_cancel_and_ignores_late_response` | Delete the legacy client timeout state. Current resumable timeout is an explicit `AcpAbortPendingRequest` with typed `agent_interaction_timeout` and atomic cleanup, covered by wrapper owner/abort tests. |

### Inline tests inside `acp_legacy`

The five `client.rs` inline tests and four `compat.rs` inline tests are duplicate
variants of permission retention/collision/line-limit cases already addressed above.
They run three times because the module is path-included by `acp_integration`,
`acp_session`, and `service`. Delete all nine; retain only the production accumulator
line-limit test.

## Dependencies and risks

1. Land the three production-path assertions first or in the same revision:
   permission option aliases, protocol-version mismatch cleanup, and boolean config
   values.
2. Item 28 is the authority for permission callback timeout/default behavior. Item
   81 must not change its default or recreate client reply routing.
3. Item 8 is the authority for native ACP filesystem/terminal methods. Browser may
   explicitly return method-not-found where it has no host callback transport; do
   not force artificial native/browser feature parity by copying handlers.
4. Removing `native-sidecar/src/json_rpc.rs` is safe by current search, but include a
   final repository-wide search because it is a companion cleanup outside the
   tracker row's originally named directory.
5. Deleting `acp_session.rs` removes timing-sensitive Tokio tests. Their timeout
   constants must remain covered by sidecar/core tests, not by a new sleep-based
   suite.
6. The unknown-frame behavior is a real strictness question, not a reason to keep
   the legacy implementation. Track it independently if the main item should remain
   a small cleanup revision.

## Small proposed diff sequence

1. Extend production-path coverage without deleting anything: add the permission
   alias table at `permission_result`, add resumable initialize-version mismatch and
   cleanup coverage at `AcpCore::feed_agent_output`, and extend the shared behavior
   config test with a boolean `value`.
2. Run the legacy suites and the three new assertions once. This establishes both
   the behavior being retired and the replacement coverage in the same revision.
3. Remove the two integration-test roots, both legacy test directories, the
   `service.rs` path imports, and the stale native-sidecar `CLAUDE.md` fixture bullet.
   Run `cargo test -p agentos-native-sidecar --test service` to prove service
   behavior never depended on the harness.
4. Remove `native-sidecar/src/json_rpc.rs` and its declaration from `lib.rs`; run the
   repository-wide search guard before the compile gates.
5. Run shared-core, native extension, native/browser conformance, workspace check,
   formatting, and diff checks; then mark Item 81 complete in the tracking document.

## Before/after validation checklist

### Before deletion

- [x] `cargo test -p agentos-native-sidecar --test acp_integration` (23/23)
- [x] `cargo test -p agentos-native-sidecar --test acp_session` (33/33)
- [x] The pre-deletion broad native run included the `service` binary and confirmed the nine
  legacy inline tests currently ran there despite no service usage.
- [x] Add and pass the three production-path assertions against the pre-deletion
  tree:
  - `cargo test -p agentos-sidecar --lib permission_results_preserve_adapter_option_ids_for_all_reply_aliases`
  - `cargo test -p agentos-sidecar-core --lib resumable_create_session_rejects_protocol_version_mismatch_and_cleans_up`
  - `cargo test -p agentos-sidecar-core --lib successful_config_updates_state_and_rejects_malformed_options`

### After deletion

- [x] The same three production-path assertions pass.
- [x] `cargo test -p agentos-sidecar-core --lib` (78/78)
- [x] `cargo test -p agentos-sidecar-core --test acp_conformance` (8/8)
- [x] `cargo test -p agentos-sidecar --lib` (12/12)
- [x] `cargo test -p agentos-sidecar --test acp_extension` (2/2)
- [x] `cargo test -p agentos-sidecar --test acp_wrapper_conformance` (15/15)
- [x] The native `service` test binary builds and its post-deletion test inventory contains no legacy ACP tests.
- [x] `cargo check -p agentos-native-sidecar`
- [x] `cargo check --workspace`
- [x] `cargo fmt --all --check`
- [x] `git diff --check`
- [x] `rg -n 'acp_legacy|native_sidecar::json_rpc|mod json_rpc' crates/native-sidecar`
  returns no legacy native-sidecar hit (the current shared-core `json_rpc` module is
  expected and remains).

## Implementation result

Revision `sqnqyqws` removes the test-only ACP client/session implementation, its
duplicated permission retention and timeout policy, the two legacy integration
roots, and the unused native typed JSON-RPC codec. The real native extension and
shared core now directly cover the only three contracts worth retaining. No
runtime state machine or compatibility policy moved into another layer.

## Proposed completion statement

Item 81 is complete when the three retained production contracts pass at their
authoritative layers, the path-included legacy harness and unused native typed codec
are gone, the service suite no longer compiles unrelated ACP state, and all shared
core/native/browser ACP gates remain green.
