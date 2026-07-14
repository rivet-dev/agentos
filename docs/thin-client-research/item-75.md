# Item 75 research: make missing ACP sessions a shared sidecar error

Status: implementation-ready research only. This note does not modify
production code, tests, or the tracker.

Inspected on **2026-07-14** at revision **`59e352ee0605`**. Tracker anchors are
`docs/thin-client-migration.md:121` (issue inventory), current line 203
(pending status), and current line 290 (before/after/complete checklist).

## Recommendation

Add `AcpCoreError::SessionNotFound(String)` to the shared native/browser ACP
core, give it the stable code `session_not_found`, and use it for every
authoritative session lookup that currently constructs
`InvalidState("unknown ACP session ...")`.

Priority: **P1**. Confidence: **high**.

No protocol schema or client behavior change is required. `AcpErrorResponse`
already transports an arbitrary string code and message. The Rust client
already maps exactly `session_not_found` to its public
`ClientError::SessionNotFound`; TypeScript already preserves the exact code on
the thrown error.

## Original issue and root cause

After Item 34 moved native and browser ACP behavior into
`agentos-sidecar-core`, the real Rust test
`session_surface_create_prompt_events_close` reaches the sidecar for
`prompt("nope", "x")` and receives:

```text
ClientError::Kernel {
    code: "invalid_state",
    message: "unknown ACP session nope",
}
```

The public Rust contract and existing test expect
`ClientError::SessionNotFound("nope")`.

`crates/client/src/session.rs:1285-1308` is already correct: it sends the
session request without a client registry gate and maps only the stable
sidecar code `session_not_found`. Expanding that client match to inspect
`invalid_state` message text would duplicate sidecar semantics in the SDK and
would misclassify unrelated invalid states.

The source defect is `crates/agentos-sidecar-core/src/lib.rs:29-60`:
`AcpCoreError` has no session-not-found variant, so every shared-core missing or
cross-owner lookup is forced into `InvalidState`.

## Exact current paths

### Shared core taxonomy

`crates/agentos-sidecar-core/src/lib.rs:29-87` defines `AcpCoreError`, maps its
variants to stable codes, formats messages, and serializes them through
`error_response` at lines 95-101. Add the new semantic variant here.

The authoritative missing-session constructions in
`crates/agentos-sidecar-core/src/engine.rs` are currently:

- `get_session_state`, lines 981-994 (construction at 986-987);
- resumable `begin_session_request`, lines 1984-2005 (construction at
  1998-1999);
- resumable request completion, lines 2399-2405;
- `begin_resumable_adapter_restart`, lines 3011-3023;
- blocking `session_request`, lines 3504-3523 (construction at 3516-3517);
- blocking request completion, lines 3623-3632; and
- `prepare_session_config`, lines 3920-3932.

All seven emit the same `unknown ACP session <id>` message as generic
`invalid_state`. Replace those exact constructions. Keep genuinely different
state-machine failures as `InvalidState`, including "session was removed during
adapter restart", missing pending interactions, busy sessions, malformed JSON,
and adapter restart failures.

`close_session` and `begin_close_session` intentionally return idempotent
success for a missing or cross-owner session. Do not change that contract, and
do not mechanically replace unrelated `InvalidState` values merely because
they mention session cleanup or adapter failure.

### Native and browser serialization

Both adapters already turn shared-core errors into ACP response payloads:

- native: `crates/agentos-sidecar/src/acp_extension.rs:497` and the resumable
  response paths call `agentos_sidecar_core::error_response`;
- browser: `crates/agentos-sidecar-browser/src/lib.rs:187-205` calls the same
  function after `dispatch_resumable`.

Therefore the shared variant automatically gives both adapters the same code.
No browser error-enum change is needed.

One native host conversion must be updated:
`sidecar_to_core_error` at
`crates/agentos-sidecar/src/acp_extension.rs:1812-1846` currently converts an
existing native `SidecarError::SessionNotFound(session_id)` back into shared
`InvalidState`. Map it directly to `AcpCoreError::SessionNotFound(session_id)`.

The older native-only `error_code` at `acp_extension.rs:3047-3063` and native
sidecar error mapping already use `session_not_found`; preserve them.

### Client consumption

Rust decoding at `crates/client/src/session.rs:1592-1621` preserves an ACP error
as `ClientError::Kernel { code, message }`. Then
`send_session_request_with_text` at lines 1285-1308 maps the exact
`session_not_found` code to `ClientError::SessionNotFound(session_id)`.

Do not add a client session-map lookup, string match, compatibility alias, or
mapping of every `invalid_state` error.

TypeScript decoding at `packages/core/src/agent-os.ts:2543-2557` constructs an
`Error` with `error.code = response.val.code`. Its flat prompt/state/config APIs
forward to the sidecar, so it will preserve `session_not_found` without a
production edit.

### Protocol and generated bindings

The source schema at
`crates/agentos-protocol/protocol/agent_os_acp_v1.bare:198-201` already defines
`AcpErrorResponse { code: str, message: str }`; it does not enumerate error
codes. Rust includes the build-generated definitions through
`crates/agentos-protocol/src/generated.rs`, and the checked-in TypeScript
binding at `packages/core/src/sidecar/agentos-protocol.ts:908-923` already reads
and writes both fields as strings.

Therefore **do not edit the BARE schema, generated Rust output, or generated
TypeScript protocol file**. Regeneration would produce no semantic change and
would incorrectly enlarge this revision.

## Exact implementation

### `crates/agentos-sidecar-core/src/lib.rs`

Add a variant carrying the session ID, not preformatted message text:

```rust
pub enum AcpCoreError {
    SessionNotFound(String),
    // existing variants...
}
```

Map it in `code()`:

```rust
AcpCoreError::SessionNotFound(_) => "session_not_found",
```

Format it in `Display`:

```rust
AcpCoreError::SessionNotFound(session_id) => {
    write!(f, "unknown ACP session {session_id}")
}
```

Keep the current message so logs, security assertions, and callers retain useful
context. Storing only the ID prevents call sites from inventing different
messages for the same semantic code.

Optionally add a private engine helper such as
`unknown_session(session_id: &str) -> AcpCoreError` if it materially shortens
the seven call sites; it must only construct the typed variant and must not
perform policy or message classification.

### `crates/agentos-sidecar-core/src/engine.rs`

Replace the seven exact `InvalidState(format!("unknown ACP session ..."))`
constructions with `AcpCoreError::SessionNotFound(session_id.to_owned())`.

Missing and cross-owner lookups must deliberately produce the same variant,
code, and message. This preserves the existing no-existence-oracle property:
an attacker still cannot distinguish "does not exist" from "exists under
another exact owner".

### `crates/agentos-sidecar/src/acp_extension.rs`

Change only this conversion arm:

```rust
SidecarError::SessionNotFound(session_id) => {
    AcpCoreError::SessionNotFound(session_id)
}
```

Do not modify the ACP wire schema, native/browser dispatch wrappers, client
session registries, or generated protocol bindings.

## Risks and exact guardrails

- **Existence leak:** absent and cross-owner session IDs must still produce the
  same code and the same requested-ID message. Never classify cross-owner as
  `unauthorized`.
- **Over-broad replacement:** change only the seven constructions whose exact
  message is `unknown ACP session ...`, plus the native conversion arm. Busy
  sessions, malformed adapter data, missing pending interactions, restart
  failures, and cleanup failures retain their current codes.
- **Closed-session semantics:** the lookup closures also classify an internal
  record with `closed == true`; keep that behavior typed as
  `session_not_found`. Close itself remains idempotent.
- **Completion race:** both completion lookups can observe that the target was
  removed while a request was in flight. `session_not_found` is still the
  correct caller-visible result because the authoritative target no longer
  exists; do not invent a client retry or compatibility state machine.
- **Exhaustive matches:** Item 36 has already added `Context` and `Cleanup` to
  `AcpCoreError`. Add the new arms without changing delegated context codes or
  the `cleanup_failed` aggregate code.

## Tests

### Before evidence

Use the existing real regression in
`crates/client/tests/session_e2e.rs:142-182`. Against Item 34
(`ac77fa88`), `prompt("nope", "x")` fails its `SessionNotFound` assertion and
prints the actual `ClientError::Kernel { code: "invalid_state", message:
"unknown ACP session nope" }`.

Record that focused failing command and parent revision in the tracker. Do not
weaken the test to accept either error.

### Shared-core unit coverage

Update `error_codes_are_stable` in
`crates/agentos-sidecar-core/src/lib.rs:107-124` to assert both:

```rust
assert_eq!(AcpCoreError::SessionNotFound("s1".into()).code(), "session_not_found");
assert_eq!(AcpCoreError::SessionNotFound("s1".into()).to_string(),
           "unknown ACP session s1");
```

Update and strengthen these engine tests:

- `get_session_state_enforces_ownership` at `engine.rs:4849-4861`;
- `session_request_enforces_ownership_without_side_effects` at
  `engine.rs:5285-5301`;
- `resumable_session_prompt_enforces_ownership` at
  `engine.rs:6887-6902`; and
- the real-agent ownership assertion at
  `crates/agentos-sidecar-core/tests/real_agent_round_trip.rs:255-260`.

For state, blocking prompt, resumable prompt, and config lookup, assert a truly
absent ID and a cross-owner existing ID both return:

```text
code = session_not_found
message = unknown ACP session <requested-id>
```

Also preserve the existing no-side-effect checks: no request ID consumed, no
pending prompt, no stdin write, and the owner's session remains unchanged.

Keep unrelated `invalid_state` assertions for missing pending process routes,
wrong-owner output injection, restart failure, and malformed adapter responses.

### Native/browser wrapper parity

Update `native_and_browser_wrappers_match_full_session_lifecycle` in
`crates/agentos-sidecar/tests/acp_wrapper_conformance.rs:319-477`:

- the cross-owner state/prompt/config/cancel loop at lines 421-451 must expect
  `session_not_found` from both wrappers; and
- `state absence after close` at lines 473-477 must explicitly assert the same
  code, not only wrapper equality.

Add one never-created session request to that conformance test and assert its
response exactly equals the cross-owner response. This is the adapter-level
proof that the security property survived the taxonomy improvement.

Update `assert_indistinguishable_deny` in
`crates/agentos-sidecar/tests/acp_extension.rs:1002-1023` to expect
`session_not_found` while retaining its unknown-session message assertion.

Update the post-eviction prompt assertion in
`crates/agentos-sidecar/tests/acp_adapter_restart.rs:175-191` from
`invalid_state` to `session_not_found`. Do not change restart failure responses
that correctly remain `invalid_state`.

### Public clients

Run the existing Rust lifecycle test unchanged; it is the end-to-end proof that
the sidecar code reaches `ClientError::SessionNotFound`:

```bash
cargo test -p agentos-client --test session_e2e \
  session_surface_create_prompt_events_close -- --nocapture
```

Extend `packages/core/tests/session-config-routing.test.ts`, which already
backdoors the transport boundary without creating client lifecycle state. Feed
`_decodeAcpResponseEnvelope` an encoded `AcpErrorResponse` with code
`session_not_found` and message `unknown ACP session nope`; assert the thrown
`Error` retains both values. Use the real namespace string and
`encodeAcpResponse` from the generated binding. This is a client pass-through
contract test, not the before-failing sidecar regression; do not introduce a
TypeScript-specific error class, local registry gate, or message parser.

## Validation

```bash
cargo test -p agentos-sidecar-core
cargo test -p agentos-sidecar --test acp_wrapper_conformance -- --nocapture
cargo test -p agentos-sidecar --test acp_extension -- --nocapture
cargo test -p agentos-sidecar --test acp_adapter_restart -- --nocapture
cargo test -p agentos-client --test session_e2e \
  session_surface_create_prompt_events_close -- --nocapture
pnpm --dir packages/core exec vitest run \
  tests/session-config-routing.test.ts --reporter=verbose
cargo check --workspace
pnpm --dir packages/core check-types
git diff --check
```

The wrapper and client tests start real native processes and are the expensive
phase.

## Stack dependencies and revision boundary

- **Item 34 (`pqpkrqpt` / `ac77fa88`) is the semantic prerequisite.** It made
  `agentos-sidecar-core` authoritative for both native and browser ACP. Item 75
  must be a child revision, not folded back into Item 34.
- **Item 35 (`nnmknwoo`) has already landed in the current stack and overlaps
  `crates/client/tests/session_e2e.rs`.** Preserve its config-decoding fixture
  changes; Item 75 does not need to edit that file because both existing
  `SessionNotFound` assertions should remain unchanged.
- **Item 36 has already extended `AcpCoreError` with `Context` and `Cleanup`.**
  Preserve those variants/codes while adding `SessionNotFound`; the only overlap
  is the small exhaustive `code`/`Display` matches.
- **Items 37-74 do not provide a semantic prerequisite.** Implement Item 75 as
  its own child revision at the current stack tip; Item 41's current client
  process-tree work is unrelated.
- **Existing historical tracker entry 18.68 states this contract.** Item 75 is
  the Item-34 regression repair and should not add a second client-side
  implementation.

Use one dedicated stacked JJ revision, for example:

```text
fix(acp): preserve missing-session errors
```

Bound production/test paths to:

- `crates/agentos-sidecar-core/src/lib.rs`
- `crates/agentos-sidecar-core/src/engine.rs`
- `crates/agentos-sidecar-core/tests/real_agent_round_trip.rs`
- `crates/agentos-sidecar/src/acp_extension.rs`
- `crates/agentos-sidecar/tests/acp_extension.rs`
- `crates/agentos-sidecar/tests/acp_adapter_restart.rs`
- `crates/agentos-sidecar/tests/acp_wrapper_conformance.rs`
- `crates/client/tests/session_e2e.rs` only if the existing assertion needs
  clearer exact-variant checking
- one focused TypeScript test file, with no TypeScript production edit
- `docs/thin-client-migration.md` only for Item 75 evidence/status

No protocol, generated bindings, browser production adapter, runtime, VFS,
package, actor, or public client implementation file belongs in this revision.
