# Item 64 research — make cron rejection codes sidecar-owned

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 64 tracker status.

## Recommendation

Make `CronSchedulerError` in `agentos-native-sidecar-core` the single source of
truth for schedule rejection codes:

- `InvalidSchedule` -> `invalid_schedule`
- `PastSchedule` -> `past_schedule`
- every other error returned by `CronScheduler::schedule` ->
  `cron_schedule_failed`

Expose those as shared constants plus a `schedule_rejection_code()` method. Have
both native and browser `schedule_cron` handlers use that method when building a
`RejectedResponse`. Remove the `[invalid_schedule]` and `[past_schedule]`
markers from human-readable messages.

Then delete all client interpretation:

- TypeScript releases any provisional host callback and rethrows the original
  `SidecarRequestRejected` object;
- Rust releases any provisional host callback and returns
  `ClientError::Kernel { code, message }` with the exact wire values; and
- the legacy TypeScript `InvalidScheduleError` / `PastScheduleError` classes and
  Rust `ClientError` variants are removed.

This keeps parsing, time comparison, semantic classification, and error coding
in one shared sidecar implementation. Clients retain only legitimate host-side
callback cleanup.

Priority: **P1**. Confidence: **high**. The scheduler already has the correct
typed variants, both divergence points and both client remappers are explicit,
and no wire schema change is required.

## Original issue and exact behavior

### Shared scheduler already knows the semantic error

`CronSchedulerError` at
`crates/native-sidecar-core/src/cron.rs:58-90` contains distinct
`InvalidSchedule(String)` and `PastSchedule(String)` variants.

`CronScheduler::schedule` at current lines 225-279:

- parses through `parse_schedule` at lines 739-764, which returns
  `InvalidSchedule`; and
- returns `PastSchedule` at lines 233-236 when a one-shot timestamp has no next
  run after the sidecar's authoritative clock.

The shared implementation therefore has all required information. Its current
`Display` implementation encodes machine semantics into message text:

```rust
"[invalid_schedule] invalid cron schedule: ..."
"[past_schedule] one-shot cron schedule is in the past: ..."
```

Those markers exist only because the adapters discard the enum variant.

### Native adapter flattens both to `invalid_state`

`NativeSidecar::schedule_cron` at
`crates/native-sidecar/src/service.rs:1498-1538` calls
`.map_err(cron_scheduler_error)`. The helper at current lines 3678-3680 converts
every `CronSchedulerError` into `SidecarError::InvalidState(error.to_string())`.
The central `error_code` mapping in
`crates/native-sidecar/src/execution.rs:24312-24328` consequently emits
`invalid_state` for both invalid and past schedules.

### Browser adapter flattens both differently

`BrowserWireDispatcher::schedule_cron` at
`crates/native-sidecar-browser/src/wire_dispatch.rs:324-349` catches every
scheduler error and emits `cron_schedule_failed` directly. Thus the same shared
scheduler result produces a different generic code depending on adapter.

### TypeScript reinterprets message text

`CronManager.schedule` at
`packages/core/src/cron/cron-manager.ts:108-146` correctly owns provisional
callback allocation and release. Its catch block then calls
`normalizeScheduleError` at current lines 401-408. That helper stringifies any
unknown error, scans for the two bracketed message markers, and replaces the
transport error with client-authored classes from
`packages/core/src/cron/errors.ts`.

This loses the original `SidecarRequestRejected.code`, request ID, ownership,
and response envelope. It also permits unrelated error text containing a marker
to be misclassified as a schedule rejection.

The classes are re-exported through:

- `packages/core/src/cron/index.ts:1-12`; and
- `packages/core/src/index.ts:3-8`.

`SidecarRequestRejected` is already exported from the core root at
`packages/core/src/index.ts:43`, so no replacement client error type is needed.

### Rust reinterprets both code substrings and message text

`AgentOs::schedule_cron` at `crates/client/src/cron.rs:647-715` releases a
provisional callback on failure, then calls `cron_rejected`. That helper at
current lines 941-953 uses `contains`, not equality, on both the code and the
message marker before replacing the wire rejection with
`ClientError::InvalidSchedule` or `ClientError::PastSchedule`.

The same helper is also used for wake, completion, list, cancel, export, and
import rejections at current lines 492, 598, 741, 800, 871, and 895. Those call
sites pass an empty schedule string, proving this is copied schedule policy in a
generic cron rejection converter rather than operation-specific transport
handling.

The two client-owned variants live at
`crates/client/src/error.rs:39-45`. Replacing a `RejectedResponse` with either
variant discards the authoritative sidecar code and message.

## Exact shared-sidecar replacement

### `crates/native-sidecar-core/src/cron.rs`

Add stable shared constants near `CronSchedulerError`:

```rust
pub const CRON_INVALID_SCHEDULE_ERROR_CODE: &str = "invalid_schedule";
pub const CRON_PAST_SCHEDULE_ERROR_CODE: &str = "past_schedule";
pub const CRON_SCHEDULE_FAILED_ERROR_CODE: &str = "cron_schedule_failed";
```

Add an operation-specific mapping on the enum:

```rust
impl CronSchedulerError {
    pub const fn schedule_rejection_code(&self) -> &'static str {
        match self {
            Self::InvalidSchedule(_) => CRON_INVALID_SCHEDULE_ERROR_CODE,
            Self::PastSchedule(_) => CRON_PAST_SCHEDULE_ERROR_CODE,
            Self::InvalidArgument(_)
            | Self::JobLimit
            | Self::UnknownRun(_)
            | Self::CounterExhausted(_) => CRON_SCHEDULE_FAILED_ERROR_CODE,
        }
    }
}
```

The fallback is deliberately operation-specific. `CronSchedulerError` is also
used by wake/import/completion; this item should not invent a new public code
taxonomy for those operations.

Change only the two semantic `Display` arms to human messages without machine
markers:

```text
invalid cron schedule: <schedule>
one-shot cron schedule is in the past: <schedule>
```

Do not add code parsing, schedule parsing, or wall-clock logic to either adapter.

### `crates/native-sidecar-core/src/lib.rs`

Export the three constants with `CronSchedulerError` and `CronScheduler` from
the existing `cron` re-export block near current lines 37-42. The method itself
is available through the exported enum.

### Native adapter

In `NativeSidecar::schedule_cron` at
`crates/native-sidecar/src/service.rs:1498-1538`, replace the
`.map_err(cron_scheduler_error)?` call with a match:

```rust
let response = match scheduler.schedule(payload, unix_time_ms()) {
    Ok(response) => response,
    Err(error) => {
        return Ok(DispatchResult {
            response: self.reject(
                request,
                error.schedule_rejection_code(),
                &error.to_string(),
            ),
            events: Vec::new(),
        });
    }
};
```

Keep `cron_scheduler_error` at current lines 3678-3680 for non-schedule cron
operations. Do not add cron variants to the global `SidecarError` enum or teach
the global `error_code` function schedule policy; this is a bounded
`ScheduleCronRequest` result.

### Browser adapter

In `BrowserWireDispatcher::schedule_cron` at
`crates/native-sidecar-browser/src/wire_dispatch.rs:324-349`, replace the hard
coded `"cron_schedule_failed"` with
`error.schedule_rejection_code()`. Its existing `rejected(...)` helper already
preserves the selected code and `error.to_string()`.

The two adapter handlers should contain no match over individual schedule
variants. The shared enum method is the only classification table.

## Exact client deletions

### TypeScript

In `packages/core/src/cron/cron-manager.ts`:

1. Remove the import of `InvalidScheduleError` and `PastScheduleError` near the
   top of the file.
2. Keep the `try/catch` in `CronManager.schedule` because releasing an allocated
   callback ID is host-only state the sidecar cannot access.
3. Change `throw normalizeScheduleError(options.schedule, error)` to
   `throw error` after that cleanup.
4. Delete `normalizeScheduleError` at current lines 401-408.

Delete `packages/core/src/cron/errors.ts`. Remove its exports from
`packages/core/src/cron/index.ts` and `packages/core/src/index.ts`.

Do not create replacement core classes. `SidecarProcess.scheduleCron` in
`packages/runtime-core/src/sidecar-process.ts:748-768` already uses the shared
protocol client, whose response validation throws `SidecarRequestRejected` with
the exact code, message, request ID, ownership, and response.

Update `packages/core/tests/public-api-exports.test.ts` to remove the two legacy
imports and their export test. Retain the existing assertion that
`SidecarRequestRejected` is exported.

### Rust

In `crates/client/src/error.rs`, delete `ClientError::InvalidSchedule` and
`ClientError::PastSchedule` at current lines 39-45.

In `crates/client/src/cron.rs`:

1. Replace `cron_rejected(rejected, schedule)` with a converter that accepts only
   `RejectedResponse` and returns the exact `ClientError::Kernel { code,
   message }`.
2. Remove the `schedule` parameter and both substring/message checks from
   current lines 941-953.
3. Update all seven call sites (schedule plus wake, completion, list, cancel,
   export, and import) to pass only the rejected response.
4. Leave callback release at current lines 694-698 unchanged.

A local helper is fine, although using an existing crate-level
`rejected_to_error` is preferable only if it can be shared without a broader
module refactor. Do not add Rust schedule code constants or a second client
classification table.

## Before and after tests

### Before validation

TypeScript already has the relevant characterization test at
`packages/core/tests/cron-manager.test.ts:217-237`. It injects errors whose only
semantic signal is a bracketed message marker and asserts that CronManager
replaces them with the two public client classes. Extend it temporarily to
assert the thrown objects are not the injected errors, recording the loss of
identity before rewriting the test.

Add a temporary Rust unit test beside the existing tests in
`crates/client/src/cron.rs:959+` named, for example,
`cron_rejected_parses_substrings_and_discards_wire_rejection`. Cover:

- a code such as `prefix_invalid_schedule_suffix` being accepted because the
  client uses `contains`;
- a generic code plus `[past_schedule]` in its message being remapped; and
- the original code/message being absent from the returned variants.

For adapter divergence, add the same invalid and past requests to the native and
browser test seams before the production edit. The pre-change observations are:

| Adapter | Invalid schedule code | Past one-shot code | Message |
|---|---|---|---|
| Native | `invalid_state` | `invalid_state` | contains bracketed marker |
| Browser | `cron_schedule_failed` | `cron_schedule_failed` | contains bracketed marker |

Record those exact results in the Item 64 tracker, then update the same tests to
the new conformance expectations.

### Shared scheduler tests after the change

In the `crates/native-sidecar-core/src/cron.rs` test module, add
`schedule_rejection_codes_are_stable`:

- construct `InvalidSchedule`, `PastSchedule`, and one fallback variant;
- assert exact `schedule_rejection_code()` constants;
- assert invalid/past messages still include the supplied schedule; and
- assert neither message contains `[` or a machine-code marker.

Keep the existing grammar test around current lines 987-1010. It proves the
parser and sidecar clock still select the correct enum variants.

### Native/browser adapter conformance

Add `crates/native-sidecar/tests/cron.rs` using the existing helpers in
`crates/native-sidecar/tests/support/mod.rs`. Authenticate, open a VM, and send
two `ScheduleCronRequest`s:

- malformed `"not a schedule"` -> exact code `invalid_schedule`;
- fixed past `"2020-01-01T00:00:00Z"` -> exact code `past_schedule`.

For each, assert the message includes the submitted schedule and contains no
bracketed marker. Also add one invalid non-schedule field (for example an empty
job ID) and assert the shared fallback `cron_schedule_failed` so native/browser
cannot diverge again for the same scheduling operation.

Add the identical three cases near the cron registry tests in
`crates/native-sidecar-browser/tests/wire_dispatch.rs:594-713`. Use the same
request payloads and exact code assertions. No JS/WASM guest execution is needed
for either adapter suite.

### Thin-client pass-through tests

Rewrite `packages/core/tests/cron-manager.test.ts:217-237` as
`passes_sidecar_schedule_rejections_through unchanged`:

- inject a real `SidecarRequestRejected` with `invalid_schedule`, then another
  with `past_schedule`;
- assert rejection by object identity (`rejects.toBe(error)`), exact code, and
  exact response envelope; and
- use a callback action in one case to retain coverage that provisional
  host-only callback state is released before rethrow.

No TypeScript test should import or instantiate the removed legacy classes.

In `crates/client/src/cron.rs`, replace the temporary remapping test with
`cron_rejected_preserves_exact_code_and_message`. Assert strict equality for a
`RejectedResponse { code: "invalid_schedule", message: ... }`, and add a
non-semantic code containing the substring to prove it is not reclassified.

Update `crates/client/tests/cron_grammar_e2e.rs:59-86` to match:

```rust
Err(ClientError::Kernel { code, message })
    if code == "invalid_schedule" && message.contains(expr)
```

and the equivalent exact `past_schedule` assertion. This retains real-sidecar
grammar and one-shot coverage while proving Rust receives the authoritative
code unchanged.

## Risks and dependencies

- **No protocol regeneration is needed.** `RejectedResponse` already carries
  stable `code` and `message` fields.
- **Client callback cleanup must remain.** Moving schedule semantics out of the
  client does not move TypeScript functions or Rust closures into the sidecar.
- **Use equality only in tests/consumers.** The sidecar emits exact codes; no
  production client should use `contains`, inspect messages, or rebuild errors.
- **Keep fallback scope narrow.** `cron_schedule_failed` covers other errors
  from `CronScheduler::schedule`. Do not recode wake/import/completion errors in
  this revision.
- **Item 63 is compatible but independent.** TypeScript
  `SidecarRequestRejected` is already structured. Item 63's terminal-process and
  ACP error work does not block Item 64.
- **Item 56 is independent.** Reliable cron dispatch/ack changes the async run
  lifecycle, not synchronous schedule validation.
- Removing the two TypeScript exports and Rust enum variants is a public API
  break, but this repository explicitly ships client, sidecar, and protocol in
  same-version lockstep with no backward-compatibility guarantee. Keeping the
  classes would preserve duplicated policy and contradict the requested thin
  boundary.
- Use a fixed old timestamp for adapter tests. Do not use a timestamp only a few
  milliseconds in the past/future, which would make the test clock-sensitive.

## Dedicated `jj` revision boundary

Use one dedicated stacked revision containing only:

- `crates/native-sidecar-core/src/{cron.rs,lib.rs}`;
- `crates/native-sidecar/src/service.rs` and the focused native cron test;
- `crates/native-sidecar-browser/src/wire_dispatch.rs` and its focused tests;
- `packages/core/src/cron/{cron-manager.ts,index.ts}`, deletion of
  `packages/core/src/cron/errors.ts`, root export cleanup, and focused TS tests;
- `crates/client/src/{cron.rs,error.rs}` and focused Rust tests; and
- the Item 64 tracker checklist/status update after validation.

Do not include Item 56 dispatch reliability, Item 63 error-type work, protocol
schema edits, or general cron error-taxonomy refactors. Verify the shared
working-copy diff before describing and stacking the revision.

Focused validation commands:

```sh
cargo test -p agentos-native-sidecar-core cron
cargo test -p agentos-native-sidecar --test cron
cargo test -p agentos-native-sidecar-browser --test wire_dispatch cron_schedule_rejection
cargo test -p agentos-client cron
pnpm --dir packages/core test -- cron-manager.test.ts public-api-exports.test.ts
cargo check --workspace
pnpm check-types
```

The final tracker evidence should name the pre-change TS/Rust remapping tests,
the observed native/browser generic codes, the shared code-selection test, both
adapter conformance tests, both client pass-through tests, and the dedicated
`jj` revision ID.
