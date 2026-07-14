# Item 59 research — make finite exec stdin sidecar-atomic

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 59 tracker status.

## Recommendation

Add `initialStdin: optional<data>` and
`closeStdinAfterInitial: optional<bool>` to the existing wire
`ExecuteRequest`. Define the sidecar contract as:

- if `initialStdin` is present, write those bytes after process creation;
- if `closeStdinAfterInitial` is explicitly `true`, deliver EOF after the
  optional initial write;
- do not return `ProcessStarted` until both requested operations succeed; and
- if either operation fails after launch, force-abort the process in the sidecar
  and return the original stdin-stage rejection.

TypeScript and Rust finite `exec` then send one `Execute` request containing the
caller's optional stdin and `closeStdinAfterInitial: true`. They delete their
subsequent `WriteStdin` and `CloseStdin` requests. Raw `spawn` and shell retain
the separate streaming write/close protocol because they return a live handle
and necessarily accept input after start.

This is preferable to adding client-side cleanup. The sidecar is the only layer
that can make process creation, initial input, EOF, and rollback one lifecycle.
It also avoids exposing a process ID before finite-input initialization is known
to be valid.

`closeStdinAfterInitial: true` is not a client-authored default. It is the
explicit wire representation of the finite `exec` API's existing promise to
deliver EOF. It must be separate from `keepStdinOpen`: that existing field
controls guest-runtime stdin liveness (`AGENTOS_KEEP_STDIN_OPEN`) and does not
currently close the kernel stdin endpoint. Normal spawn continues to preserve
`keepStdinOpen` omission, `true`, and `false` exactly as the caller supplied
them. Neither new field is defaulted by either client or the sidecar.

Priority: **P1**. Confidence: **high**. The unsafe sequences and all native and
browser process-control implementations are in this repository. The main risk
is protocol-wide mechanical fallout from adding one generated struct field, not
uncertainty about ownership or behavior.

Tracker anchors in the current tree:

- issue: `docs/thin-client-migration.md:105`;
- status: `docs/thin-client-migration.md:192`; and
- before/after/completion checklist: `docs/thin-client-migration.md:284`.

## Original issue and exact failure

### TypeScript

`NativeSidecarKernelProxy.exec` and `execArgv` at
`packages/core/src/sidecar/rpc-client.ts:394-449` currently do this:

```ts
const proc = await this.spawn(...); // Execute has succeeded
if (options?.stdin !== undefined) {
	await proc.writeStdin(options.stdin); // a second request
}
await proc.closeStdin(); // a third request
return await proc[processCompletion];
```

`spawn` installs the returned process into `trackedProcesses` and
`trackedProcessesById` at current lines 454-529 and 777-804. If either stdin
request rejects, finite `exec` returns the rejection without a kill and without
waiting for the process. The internal event pump still knows the entry, but the
caller never received the `ManagedProcess` handle, so it cannot supervise or
terminate the live process. It remains until it exits independently, the VM is
disposed, or another lifecycle path happens to clean it up.

The existing tests in
`packages/core/tests/process-event-ordering.test.ts:141-196` already inject a
write rejection and an EOF rejection. They prove the original error propagates,
but they currently push a synthetic exit to finish cleanup and never require a
kill. Those tests are the best pre-change characterization seam.

### Rust

`AgentOs::exec_request` at `crates/client/src/process.rs:202-294` calls
`send_execute`, receives `(ProcessStartedResponse, WireEventSubscription)`, then
issues an optional `WriteStdinRequest` at current lines 231-250 and an
unconditional `CloseStdinRequest` at lines 251-266.

Any transport failure, `RejectedResponse`, or unexpected response in those two
blocks returns before `collect_exec_events`. The local `process_id` is lost and
the event subscription is dropped. Unlike `spawn`, finite Rust exec never puts
this process in `AgentOsInner.processes`, so the caller has neither a handle nor
a registry entry from which to kill it.

The later event-stream-lag branch at lines 279-286 correctly calls
`abort_wire_process_after_route_failure`, but that protection is unreachable
for the earlier stdin-control failures.

### Why the protocol permits the gap

`ExecuteRequest` in
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:373-387` has launch,
PTY, stdin-lifetime, timeout, and capture fields, but no initial input. Initial
input and EOF therefore require the separate request types at current lines
389-402.

The native sidecar implements the operations independently in
`crates/native-sidecar/src/execution.rs:3751-4157`, `4209-4250`, and
`4252-4280`. The browser sidecar does the same in
`crates/native-sidecar-browser/src/wire_dispatch.rs:1907-2097`, `2099-2130`,
and `2132-2160`. Neither adapter can currently know that the later write and EOF
belong to a finite-exec transaction whose failed setup must abort the process.

The existing sidecar/process protocol already carries the authoritative
terminal result. `ProcessExitedEvent` at
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:969-975` includes the
process ID, real `i32` exit code, optional captured output, and optional typed
rejection. Item 59 does not alter that event. It only prevents an initialization
failure from being reported as a successful `ProcessStarted` followed by an
unsupervised process.

## Boundary and documentation audit

This is sidecar-owned lifecycle behavior, not a new client abstraction. The
clients are allowed to encode and forward explicit `ExecOptions.stdin`; they
must not coordinate multiple state-changing requests or own rollback policy.
The sidecar already owns process registration, stdin endpoints, signals,
terminal events, and reaping, so it is the only layer that can guarantee that a
failed finite-input start is never exposed as a usable process.

No public TypeScript or Rust API changes:

- `ExecOptions.stdin` remains the caller input;
- `exec`/`execArgv` and `exec`/`exec_argv` retain their current result types;
- `spawn`, `writeProcessStdin`, `closeProcessStdin`, Rust process streams, and
  shell streaming controls retain their current live-handle contracts; and
- no new client default, retry, timeout, or process registry is introduced.

The user-facing docs scan found only existing semantic descriptions:

- `packages/core/README.md:83` describes `exec` at API level;
- `website/src/content/docs/docs/processes.mdx:7` describes finite exec versus
  streaming spawn;
- `website/src/content/docs/docs/architecture/processes.mdx:49-54` describes
  kernel stdin/EOF; and
- the runtime examples use the unchanged public methods.

Those claims remain correct, so Item 59 should not churn website or example
docs. Document the atomic initialization contract in comments beside the new
BARE field and sidecar helper, and update only the migration tracker after tests.

## Exact replacement contract

Extend the schema, preferably adjacent to `keepStdinOpen`:

```bare
type ExecuteRequest struct {
  # existing fields ...
  initialStdin: optional<data>
  closeStdinAfterInitial: optional<bool>
  keepStdinOpen: optional<bool>
  timeoutMs: optional<u64>
  captureOutput: optional<bool>
}
```

The sidecar operation order must be exactly:

1. validate and resolve the Execute request;
2. create and register the active process internally;
3. write `initialStdin` when present, including preserving explicit empty input;
4. close stdin when `closeStdinAfterInitial == Some(true)`;
5. only then emit lifecycle busy state and return `ProcessStarted`;
6. on step 3 or 4 failure, issue `SIGKILL`/browser abort, retain the process in
   sidecar supervision until it is reaped, log any cleanup failure with the
   process ID, and return the original setup error instead of cleanup's error.

The native lifecycle emission in step 5 is itself fallible. Treat it as part of
the same pre-response transaction: if `emit_lifecycle(Busy)` fails after the
process was inserted, abort the process and preserve the lifecycle error as the
primary rejection. The current code already has a broader post-launch gap at
this exact line; moving stdin into Execute must not leave a rejected Execute
owning a process that no client can name. This is cleanup of the touched
transaction boundary, not client policy or a new lifecycle state machine.

Item 58's Execute transport route remains active throughout these steps. Early
output/terminal events may exist before the delayed `ProcessStarted` response;
the provisional route must preserve them and bind them to the authoritative
process ID on success. On an Execute rejection it must discard the provisional
route without manufacturing a started process. Do not add a second client event
buffer or bypass Item 58's typed `execute_wire` entry point.

Omitted or `false` `closeStdinAfterInitial` does not close stdin. Existing
`keepStdinOpen` behavior remains unchanged, preserving streaming spawn and PTY
behavior. No global stdin default should be added to `apply_execute_defaults`.

The initial bytes remain bounded by the existing request frame limit. They now
share that frame with launch metadata, so the theoretical maximum input is
smaller by the encoded Execute-field overhead than today's standalone
`WriteStdinRequest`. No exact standalone-stdin capacity is public, and an
oversized atomic request fails before launch, which is safe. Record this as a
bounded compatibility risk; do not restore client chunking or a multi-request
transaction to recover a few metadata bytes.

## Exact production edits

### Protocol and generated TypeScript

1. In
   `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:373-387`, add
   `initialStdin: optional<data>` and
   `closeStdinAfterInitial: optional<bool>` to `ExecuteRequest`, with comments
   that the bytes are written and the optional close is performed before
   `ProcessStarted`; both participate in fail-closed initialization. Do not
   reinterpret `keepStdinOpen`.
2. Regenerate `packages/runtime-core/src/generated-protocol.ts` with
   `pnpm --dir packages/build-tools build:protocol`; do not hand-edit it.
3. In `packages/runtime-core/src/request-payloads.ts:175-190`, add
   `initial_stdin?: Uint8Array` and `close_stdin_after_initial?: boolean` to the
   live Execute payload.
4. In the Execute conversion at current lines 473-506, map the bytes with
   `toExactArrayBuffer(payload.initial_stdin)` and the boolean without a truthy
   check, preserving omission as `null` for both.

Rust generated wire code is produced from the schema at build time. Adding the
fields makes every Rust `ExecuteRequest { ... }` literal require
`initial_stdin: None` and `close_stdin_after_initial: None` unless it is a
finite-exec construction. The mechanical compile-fix inventory is currently:

- `crates/agentos-sidecar/src/acp_extension.rs`
- `crates/client/src/process.rs`
- `crates/client/src/shell.rs`
- `crates/native-sidecar-browser/src/wire_dispatch.rs`
- `crates/native-sidecar-browser/tests/wire_dispatch.rs`
- `crates/native-sidecar-core/src/execution_defaults.rs`
- `crates/native-sidecar/src/service.rs`
- `crates/native-sidecar/tests/{extension,filesystem,python,security_hardening,service,signal,stdio_binary}.rs`
- `crates/native-sidecar/tests/support/mod.rs`
- `crates/sidecar-client/src/transport.rs`

Keep those edits mechanical. Do not use the new fields for cron, ACP, shell,
spawn, or internal sidecar launches as part of Item 59.

Extend `crates/native-sidecar-core/src/execution_defaults.rs` with one focused
assertion that `apply_execute_defaults` leaves both new fields unchanged for PTY
and non-PTY requests. It must neither synthesize initial bytes nor turn an
omitted close flag into `false`/`true`.

### TypeScript transport serializer

In `packages/runtime-core/src/sidecar-process.ts:1453-1517`:

- add `initialStdin?: string | Uint8Array` to `SidecarProcess.execute` options;
- add `closeStdinAfterInitial?: boolean`, preserving false and omission;
- encode strings as UTF-8 exactly like current `writeStdin`, serialize the
  result as `initial_stdin`, and preserve omission; and
- leave `writeStdin` and `closeStdin` intact for live spawn/shell handles.

Extend
`packages/runtime-core/tests/request-payloads.test.ts:350-384` and
`packages/runtime-core/tests/sidecar-process.test.ts:275-310` to prove exact
text/binary byte forwarding, explicit empty input, explicit `false`, and
omission for `closeStdinAfterInitial`, while proving `keepStdinOpen` is
unchanged.

### TypeScript core client

In `packages/core/src/sidecar/rpc-client.ts`:

1. Extend the private `TrackedProcessEntry` near current lines 163-182 with an
   optional `initialStdin: string | Uint8Array` and
   `closeStdinAfterInitial: boolean | undefined`. These are explicit request
   data, not policy. Encode strings once at the runtime-core request serializer,
   using the same UTF-8 behavior as current `writeStdin` at
   `packages/runtime-core/src/sidecar-process.ts:1519-1544`.
2. Extend the existing private internal spawn-options seam near lines 454-469
   with `initialStdin?` and `closeStdinAfterInitial?`. No separate finite-input
   marker is needed: the new close field represents finite EOF even when stdin
   itself was omitted. Do not put initial stdin back on public
   `KernelSpawnOptions`; Item 43 removes inert raw-spawn stdin.
3. Have `exec` and `execArgv` pass their explicit `options.stdin` as
   `initialStdin` and explicitly pass `closeStdinAfterInitial: true` for this
   finite operation. Runtime-core performs the one UTF-8 encoding at the wire
   seam. Do not set or reinterpret `streamStdin`/`keepStdinOpen`.
4. Delete both client-side `proc.writeStdin(...)` and `proc.closeStdin()` blocks.
   They should start the tracked process once and then await only
   `processCompletion`.
5. In `startTrackedProcess` at current lines 777-804, forward
   `entry.initialStdin` and `entry.closeStdinAfterInitial` when present.

An equally small private `startFiniteProcess` helper is acceptable if Item 43
has already removed the internal cast. Do not add a public finite-input builder,
retry state machine, or client kill fallback. An Execute rejection happens
before the entry is inserted into either tracking map, while a successful
Execute is tracked and observed exactly as today.

Public `AgentOs.exec`/`execArgv` in
`packages/core/src/agent-os.ts:1607-1638` already only forward to the kernel
proxy and need no behavioral logic.

### Rust client

In `crates/client/src/process.rs`:

1. Convert `options.stdin.take()` with the existing `stdin_to_bytes` before
   `send_execute` in `exec_request` at current lines 202-228.
2. Extend `send_execute` and `build_process_execute_request` at current lines
   698-743 and 890-915 with `initial_stdin: Option<Vec<u8>>` and
   `close_stdin_after_initial: Option<bool>`.
3. For finite `exec_request`, pass the converted bytes and
   `close_stdin_after_initial: Some(true)`. Preserve the existing
   `keep_stdin_open` argument independently.
4. Delete the post-start `WriteStdinRequest` and `CloseStdinRequest` blocks at
   current lines 231-266.
5. Pass both new fields as `None` from raw `spawn` and from `open_shell` in
   `crates/client/src/shell.rs`.

Keep `write_process_stdin`, `close_process_stdin`, and shell write/close methods.
They are live-handle transport operations and are not the duplicated finite
policy this item removes.

After Item 58, `send_execute` should use the typed Execute transport operation.
The new field does not change the atomic event-route requirement or introduce a
second transport path.

### Native sidecar

In `crates/native-sidecar/src/execution.rs`:

1. Extract the process mutation portions of current `write_stdin` and
   `close_stdin` into private helpers that accept `&mut VmState` plus
   `process_id`. The existing wire handlers should call the same helpers, so
   atomic Execute and streaming controls cannot diverge on JavaScript PTY,
   kernel-fd, Python, WASM, or tool behavior.
2. Add an `initialize_execute_stdin` helper that performs optional write then
   optional close against the newly inserted active process.
3. Call it in both Execute success branches:
   - the projected-tool branch after insertion around current lines 3840-3868;
   - the normal runtime branch after insertion around current lines 4129-4140.
4. Move lifecycle-busy emission and `ProcessStarted` construction after this
   initialization succeeds.
5. On an initialization **or lifecycle-busy emission** failure, release the
   mutable VM borrow, call
   `kill_process_internal(vm_id, process_id, "SIGKILL")` at current lines
   4645-4652, log a cleanup failure with both errors if it occurs, and return the
   original write/close/lifecycle `SidecarError`.

Do not duplicate the code in `ActiveExecution::write_stdin` /
`close_stdin` (current lines 3246-3278) or the kernel helpers at current lines
19458-19535. Those are already the runtime-specific source of truth.

The projected-tool path currently treats write and close as successful no-ops.
Preserve that behavior; changing tool stdin semantics is outside this item.

### Browser sidecar

In `crates/native-sidecar-browser/src/wire_dispatch.rs:1907-2097`:

1. After `start_execution_with_options` succeeds, use the returned execution ID
   to call `BrowserSidecar::write_stdin` for `initial_stdin`, then
   `BrowserSidecar::close_stdin` when
   `close_stdin_after_initial == Some(true)`.
2. Perform this before inserting `ExecutionRecord` and `process_executions` at
   current lines 2082-2095 and before returning `ProcessStarted`.
3. If either operation fails, call
   `BrowserSidecar::abort_execution(vm_id, execution_id)` from
   `crates/native-sidecar-browser/src/service.rs:2551-2580`. That method already
   force-kills and releases the browser worker/kernel execution.
4. Return `write_stdin_failed` or `close_stdin_failed` with the original primary
   message. Log abort/release errors separately; do not replace the primary
   rejection or install wire process maps for a failed start.

The separate browser wire `write_stdin`, `close_stdin`, and `kill_process`
handlers at `wire_dispatch.rs:2099-2190` remain for streaming processes.

## Before and after tests

### Before validation

TypeScript can characterize the current bug directly in
`packages/core/tests/process-event-ordering.test.ts` by extending the existing
two rejection tests:

- after a write rejection, assert `killProcess` and `closeStdin` were not called
  and `__trackingSizesForTest()` still reports one live tracked entry;
- after an EOF rejection, assert `killProcess` was not called and the same entry
  remains; and
- retain `rejects.toBe(writeError/closeError)` to prove the primary error.

These assertions pass against the pre-change code and explicitly demonstrate
why propagation alone is insufficient. Record their pre-change command/result
in the Item 59 tracker before replacing them.

Rust has no injectable `AgentOs` transport seam today. For the pre-change
checklist, use a temporary test-only extraction of the current post-start stdin
sequence in `crates/client/src/process.rs`: supply closures that return a
successful Execute/process ID followed by a write or close rejection, then
assert the function returns without invoking a kill closure and drops the event
receiver. This temporary characterization helper should not survive the final
diff; retaining a client process-orchestration abstraction would conflict with
the fix. Record both cases in the tracker.

### Retained client tests after the move

Rewrite the TypeScript ordering tests to assert:

- `exec` and `execArgv` each send one Execute containing exact stdin bytes and
  `closeStdinAfterInitial: true`, without changing `keepStdinOpen`;
- neither calls `writeStdin`, `closeStdin`, nor `killProcess`;
- an Execute `SidecarRequestRejected` is returned by identity/code/message and
  no tracking entry is installed; and
- a successful ProcessStarted plus terminal event still returns the same
  capture and callbacks.

In `crates/client/src/process.rs` unit tests near the existing
`execute_request_preserves_false_true_and_omitted_stream_stdin`, assert the
finite builder contains exact text/binary bytes plus
`close_stdin_after_initial: Some(true)`, while spawn and shell contain both new
fields as `None` and preserve `keep_stdin_open` omission/explicit values.
Use focused names such as
`finite_execute_request_embeds_input_and_close` and
`streaming_execute_omits_atomic_stdin_fields`.
Keep the existing success coverage in
`crates/client/tests/process_e2e.rs:103-169`; its text, callback, and binary
stdin cases become end-to-end proof of the new atomic request from Rust.

### Move failure ownership to sidecar tests

Add authoritative browser tests in
`crates/native-sidecar-browser/tests/wire_dispatch.rs`:

- `finite_execute_writes_input_and_closes_before_started` records one exact
  write and one close before `ProcessStarted`;
- `finite_execute_write_failure_aborts_without_process_route` injects a bridge
  write failure, expects `write_stdin_failed`, observes a kill/release, and
  proves no process snapshot or wire route remains;
- `finite_execute_close_failure_aborts_without_process_route` does the same for
  `close_stdin_failed`; and
- both failure tests assert the injected primary message survives even if an
  abort-cleanup error is also injected.

The shared `RecordingBridge` in `crates/bridge/tests/support.rs:43-108` already
records writes, closes, and kills and has error queues for start/kill. Add
matching bounded queues and `push_*_error` methods for stdin write and close so
these tests are deterministic.

Add native tests beside the stdin coverage in
`crates/native-sidecar/tests/service.rs:5393-5563`:

- a successful Execute with `initial_stdin` and
  `close_stdin_after_initial: Some(true)` proves the guest reads the bytes and
  then EOF without separate requests;
- a helper-level test inserts an `ActiveProcess` with an invalid kernel stdin
  writer fd to force the write stage to fail, then proves `SIGKILL`/terminal
  cleanup and the original `SidecarError::Kernel`;
- a second invalid-fd case omits input and forces close failure with the same
  cleanup assertion; and
- a lifecycle bridge failure after successful atomic stdin initialization also
  rejects, force-aborts, and remains sidecar-supervised until reaping; and
- retain the current separate `WriteStdin`/`CloseStdin` test to guard streaming
  spawn behavior.

`crates/native-sidecar/tests/service.rs` includes the production source modules
directly, so it can exercise a `pub(crate)` initialization helper and construct
the existing `ActiveProcess` fixture without exporting a production test hook.
Do not add a public failure-injection API.

This moves the failure-injection responsibility from client orchestration tests
to the two enforcement adapters while leaving small client serialization/parity
tests in place.

## Dependencies and risks

- **Stack after Item 43.** Do not reintroduce public raw-spawn `stdin`; use a
  private finite-exec launch field only.
- **Stack after Item 58.** Rust Execute must continue using its typed atomic
  process-event route, provisional event retention, and cancellation tombstone.
  Initial input does not make generic Execute safe.
- **Do not combine Item 60.** The shell CLI still queues truly streaming writes
  after start and needs its own terminal queue-failure behavior.
- **Do not combine Item 63.** TypeScript request rejection is already the
  exported `SidecarRequestRejected`; terminal-event error typing is separate.
- A write can partly reach the guest before failing. Cleanup must therefore be
  forceful, never retry the input, and never return `ProcessStarted`.
- **Frame-bound edge:** initial bytes and launch metadata share one bounded
  request. Oversize must fail with the existing typed frame-limit error before
  sidecar launch; do not special-case or silently truncate input.
- The native abort may remain in `active_processes` briefly until its terminal
  event is reaped. That is sidecar-owned supervision, not an orphan; tests must
  poll the normal cleanup path and prove the guest is no longer live.
- Events/output may be produced before `ProcessStarted`. Existing provisional
  Execute routing from Item 58 must retain and bind them in order. A rejected
  atomic start must drop that provisional route without publishing a PID; do not
  add a second client event buffer.
- A protocol field addition causes broad literal churn. Keep every non-finite
  initializer at `None` and reject opportunistic refactors in the same revision.

## Dedicated `jj` revision boundary

Use one dedicated stacked revision for Item 59. It should contain only:

- the schema field and regenerated TypeScript protocol;
- live payload conversion and mechanical Rust `ExecuteRequest` literal updates;
- the TypeScript and Rust finite-exec serialization simplification;
- native and browser sidecar atomic initialization/abort behavior;
- the focused client and sidecar tests above; and
- the Item 59 tracker checklist/status update after validation.

Expected behavioral paths are:

```text
crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare
packages/runtime-core/src/generated-protocol.ts              # regenerated
packages/runtime-core/src/request-payloads.ts
packages/runtime-core/src/sidecar-process.ts
packages/runtime-core/tests/request-payloads.test.ts
packages/runtime-core/tests/sidecar-process.test.ts
packages/core/src/sidecar/rpc-client.ts
packages/core/tests/process-event-ordering.test.ts
crates/client/src/process.rs
crates/client/src/shell.rs                                    # None literal only
crates/native-sidecar-core/src/execution_defaults.rs
crates/native-sidecar/src/execution.rs
crates/native-sidecar/tests/service.rs
crates/native-sidecar-browser/src/wire_dispatch.rs
crates/native-sidecar-browser/tests/wire_dispatch.rs
crates/bridge/tests/support.rs
docs/thin-client-migration.md                                 # Item 59 only
```

The schema addition also requires mechanical `initial_stdin: None` and
`close_stdin_after_initial: None` updates in the Rust `ExecuteRequest` literal
files inventoried above. Those compile fixes belong in this same revision but
must not acquire new behavior. No website, example, actor, ACP protocol, or
public client-type file should change.

Do not include Item 60 shell queue changes, Item 63 error-type work, or unrelated
process refactors. Before describing the revision, verify the diff with
`jj diff --stat` and `jj diff`; then describe/set the existing stack bookmark in
place under the workspace's shared-`@` rules.

Focused validation commands:

```sh
pnpm --dir packages/build-tools build:protocol
pnpm --dir packages/runtime-core test -- request-payloads.test.ts sidecar-process.test.ts
pnpm --dir packages/core test -- process-event-ordering.test.ts
cargo test -p agentos-client --lib finite_execute_request_embeds_input_and_close
cargo test -p agentos-client --lib streaming_execute_omits_atomic_stdin_fields
cargo test -p agentos-native-sidecar-browser --test wire_dispatch finite_execute
cargo test -p agentos-native-sidecar --test service finite_execute
cargo test -p agentos-client --test process_e2e process_surface_exec_spawn_and_snapshot
cargo fmt --all -- --check
cargo check --workspace
pnpm check-types
git diff --check
```

The final tracker evidence should name the exact pre-change TS/Rust
characterization tests, the native/browser failure tests that replace them, the
successful client parity tests, and the dedicated `jj` revision ID.
