# Item 43 research — remove inert process options and false limit knobs

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 43 tracker status.

Refreshed against the shared working tree on 2026-07-14. Overall tracker
priority: **P2**. Confidence: **high**. The false JavaScript limit controls are
individually **P1 / high confidence** because an operator can set what appears
to be a security/resource bound and receive no enforcement change. If Item 43
must retain one priority, escalating the whole item to P1 is reasonable; the
process-option-only portion remains P2.

## Recommendation

Shrink the direct process APIs to options that have an observable implementation
in the shared sidecar protocol, plus host callbacks that only a client can hold.

Remove from both clients:

- the inline-code `filePath` / `file_path` compatibility field;
- per-execution `cpuTimeLimitMs` / `cpu_time_limit_ms`;
- per-execution `timingMitigation` / `timing_mitigation` and the now-orphaned
  core/Rust `TimingMitigation` exports;
- raw-spawn `stdio` and `SpawnStdio`;
- raw-spawn `stdinFd` / `stdoutFd` / `stderrFd` and their Rust equivalents;
- inherited `stdin` and `captureStdio` / `capture_stdio` from raw spawn, where
  neither client honors them;
- the TypeScript-only public raw-spawn `pty` option.

Also remove the three advertised JavaScript-runtime overrides that are accepted,
resolved, and then never consumed by either execution adapter:

- `stdinBufferLimitBytes` / `stdin_buffer_limit_bytes`;
- `eventPayloadLimitBytes` / `event_payload_limit_bytes`; and
- `v8IpcMaxFrameBytes` / `v8_ipc_max_frame_bytes`.

Do not plumb these knobs into the runtime merely to preserve an inert API.
Native execution already owns fixed bounded defaults at the actual queue/codec
sites, while the browser adapter uses a kernel pipe and a different worker
transport. A real configurable limit would need one cross-adapter semantic
definition, including queued-byte accounting and both sides of the V8 codec.
That design does not exist today. Removing the false overrides is the smaller,
truthful thin-client behavior. Keep the implemented
`capturedOutputLimitBytes` override.

Keep:

- `env`, `cwd`, `timeout`, `onStdout` / `onStderr` on both exec and spawn;
- `stdin` and `captureStdio` on exec only;
- `streamStdin` on spawn, preserving `false`, `true`, and omission;
- every current `OpenShellOptions` field. `openShell` / `open_shell` is the
  complete cross-client terminal API and already sends the sidecar's real
  `ExecuteRequest.pty` descriptor.

Do not add sidecar fields for the removed options. CPU policy already belongs to
VM-level `limits.jsRuntime.cpuTimeLimitMs`; `filePath` and timing mitigation
belong to the separate inline-code browser runtime; host fd inheritance has no
defined meaning across a sidecar boundary; and raw spawn already exposes stdin
and output as explicit process-handle routes.

Do not remove `ExecuteRequest.pty` from the wire. TypeScript and Rust both use it
through `openShell`, which supplies stdin, EOF, output, resize, signals, and wait.
The TypeScript-only raw-spawn option exposes only initial dimensions and no
process-PTY resize operation, so removing that public duplicate is preferable to
adding the incomplete surface to Rust.

Every removed common field is accepted by a public type but is not read by
either production serializer. The only one-client field (`pty`) has a complete
parity replacement already in both clients.

## Scope of the inventory

The direct public process option bags are:

- TypeScript `ExecOptions`, `KernelSpawnOptions`, and `OpenShellOptions` in
  `packages/core/src/runtime.ts`;
- Rust `ExecOptions` and `SpawnOptions` in `crates/client/src/process.rs`, plus
  `OpenShellOptions` in `crates/client/src/shell.rs`.

The follow-up compiler review also exposed the three inert VM-scoped JavaScript
limit overrides above. They appear in TypeScript `AgentOsLimits`, Rust
`JsRuntimeLimits`, the typed VM config, and the shared resolved-limit struct,
but have no consumer outside config parsing/validation. They are included here
because they are process-runtime options exposed by both clients and match the
same rule: remove unsupported input instead of making clients or adapters
pretend to honor it.

The actor process actions expose only `env`, `cwd`, and `streamStdin`; all three
are honored. The actor has one Rust construction-site update after
`SpawnOptions` stops nesting `ExecOptions`, but its generated TypeScript action
contract does not change.

The similarly named `ExecOptions` and `TimingMitigation` in
`packages/runtime-browser` are a different, inline-code execution API. Those
fields are implemented in the browser worker and are out of scope. So are VM
limits other than the three false overrides identified above, guest Node
`child_process` stdio/fd mappings, TypeScript compiler `filePath`, and host-side
uses of Node's own `stdio` option.

## Complete option inventory

| Public option | TypeScript behavior | Rust behavior | Recommendation |
|---|---|---|---|
| exec `env` | Forwarded to `ExecuteRequest.env` | Forwarded; an empty map is omitted | Keep; empty and omitted have the same sidecar merge result |
| exec `cwd` | Forwarded only when supplied | Forwarded as `Option<String>` | Keep |
| exec `stdin` | Written after start, then EOF is awaited | Written after start, then EOF is awaited | Keep on exec only |
| exec `timeout` | Validated/truncated and sent as `timeoutMs` | Validated/truncated and sent as `timeout_ms` | Keep; native owns enforcement and browser returns typed `unsupported` |
| exec output callbacks | Host event callbacks | Host event callbacks | Keep; closures are legitimate host-only state |
| exec `captureStdio` | Defaults true and maps to `captureOutput` | Defaults true and maps to `capture_output` | Keep on exec only |
| exec `filePath` | Never read by core, proxy, or wire serializer | `file_path` exists but is never read | Remove both |
| exec `cpuTimeLimitMs` | Never read | `cpu_time_limit_ms` is never read | Remove; VM limit is authoritative |
| exec `timingMitigation` | Never read | `timing_mitigation` is never read | Remove both and delete orphaned exports |
| spawn `env` | Forwarded | Forwarded through `options.base.env` | Keep, but make it a direct Rust spawn field |
| spawn `cwd` | Forwarded | Forwarded through `options.base.cwd` | Keep, but make it a direct Rust spawn field |
| spawn `timeout` | Forwarded | Forwarded through `options.base.timeout` | Keep as a direct Rust spawn field; same adapter capability rule as exec |
| spawn output callbacks | Seed host handler sets | Seed Rust broadcast callback tasks | Keep, as direct spawn fields |
| inherited spawn `stdin` | Accepted because spawn extends exec; never read | Accepted in `base`; never read | Remove from spawn; callers use `writeProcessStdin` |
| inherited spawn `captureStdio` | Accepted; raw spawn never requests capture | Accepted in `base`; Rust sends `capture_output: None` | Remove from spawn; raw spawn is streaming |
| inherited spawn file/CPU/timing | Accepted; never read | Accepted in `base`; never read | Remove with the exec declarations |
| spawn `stdio` | `"pipe" \| "inherit"` is never read | `SpawnStdio` is never read | Remove; events are the host output interface |
| spawn fd overrides | Three numbers are never read | Three `Option<i32>` fields are never read | Remove; host fds are not guest kernel fds |
| spawn `streamStdin` | Forwarded as presence-aware `keepStdinOpen` | Forwarded with identical three-state behavior | Keep exactly as-is |
| spawn `pty` | Forwarded to native sidecar; browser returns typed unsupported | No Rust spawn field | Remove from public raw spawn; keep internal `openShell` PTY forwarding |
| shell `command`, `args` | Forwarded; omission lets sidecar choose `sh` | Forwarded identically | Keep |
| shell `env`, `cwd` | Forwarded | Forwarded | Keep |
| shell `cols`, `rows` | Forwarded as `PtyOptions` | Forwarded as `PtyOptions` | Keep |
| shell `onStderr` | Host callback route | Host callback route | Keep |
| VM JS `stdinBufferLimitBytes` | Accepted and forwarded into resolved VM limits; native stdin still uses a fixed 16 MiB local-bridge cap, while browser uses its kernel/worker path | Same wire/config behavior | Remove the false override; retain the runtime-owned bound |
| VM JS `eventPayloadLimitBytes` | Accepted and resolved; native event delivery still uses a fixed 1 MiB constant and browser does not consume the value | Same wire/config behavior | Remove the false override; do not imply adapter parity |
| VM JS `v8IpcMaxFrameBytes` | Accepted, range-checked, and resolved; both native V8 codecs still use independent fixed 64 MiB constants | Same wire/config behavior | Remove the false override; a future tunable must configure both codec sides |

## Proof of ignored and divergent fields

### TypeScript serializer

The stale public declarations are exactly
`packages/core/src/runtime.ts:145-170`; `TimingMitigation` is declared at line 2
and re-exported from `packages/core/src/types.ts:96-120`.

`NativeSidecarKernelProxy.exec` / `execArgv` at
`packages/core/src/sidecar/rpc-client.ts:393-451` consume exec-only stdin and
capture. `spawn` at lines 454-495 constructs `TrackedProcessEntry` from only:

```text
command / shellCommand / args / cwd / pty / env / streamStdin /
timeout / internal captureOutput / stdout callback / stderr callback
```

`startTrackedProcess` at lines 777-804 serializes only those values into
`execute(...)`. It never reads `filePath`, `cpuTimeLimitMs`, `timingMitigation`,
`stdio`, any fd override, spawn `stdin`, or spawn `captureStdio`.

`exec` and `execArgv` separately consume `stdin` and derive the internal
`captureOutput` value from `captureStdio`, proving those two options are real only
on the completion-returning exec path.

### Rust serializer

The stale Rust declarations are exactly
`crates/client/src/process.rs:37-118`, with public re-exports at
`crates/client/src/lib.rs:60-63`. `AgentOs::exec_request` at
`crates/client/src/process.rs:202-294` reads only `env`, `cwd`, `stdin`,
`timeout`, callbacks, and `capture_stdio`. It never reads the remaining three
`ExecOptions` fields.

`AgentOs::spawn` at `crates/client/src/process.rs:296-380` reads only:

```text
options.base.env
options.base.cwd
options.base.timeout
options.base.on_stdout
options.base.on_stderr
options.stream_stdin
```

It never reads `base.stdin`, `base.capture_stdio`, the three legacy exec fields,
`stdio`, or any fd override. `build_process_execute_request` hard-codes
`pty: None` for this path, proving the raw-spawn PTY divergence.

The wire schema at
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:368-387` has only
`env`, `cwd`, `pty`, `keepStdinOpen`, `timeoutMs`, and `captureOutput` among
these concepts. It has no `filePath`, per-execution CPU/timing field, stdio mode,
or fd override.

The sidecar behavior confirms which wire fields are real:

- shared `apply_execute_defaults` at
  `crates/native-sidecar-core/src/execution_defaults.rs:5-25` owns PTY shell and
  streaming-stdin defaults;
- native execute at `crates/native-sidecar/src/execution.rs:3751-3770` owns
  capture and timeout setup, and lines 3929-3963 create/resize the kernel PTY;
- browser execute at
  `crates/native-sidecar-browser/src/wire_dispatch.rs:1907-1950` explicitly
  returns typed `unsupported` for PTY and timeout requests, while still
  implementing ordinary process execution and capture; and
- Rust `open_shell` at `crates/client/src/shell.rs:106-153` and TypeScript
  `openShell` at `packages/core/src/sidecar/rpc-client.ts:535-558` both send the
  existing PTY descriptor.

That browser difference is an adapter capability, not an ignored client field.
Keep `timeout` and the standard `openShell` surface: both clients forward them
identically and native has authoritative implementations. Do not make either
client emulate unsupported browser behavior.

### Existing authoritative alternatives

- Per-VM JavaScript CPU enforcement is already wired through
  `AgentOs.create({ limits: { jsRuntime: { cpuTimeLimitMs }}})` and Rust
  `JsRuntimeLimits.cpu_time_limit_ms`; native execution passes the resolved VM
  value to V8.
- `packages/runtime-browser` really implements inline-code `filePath` and timing
  mitigation. Core command execution should not retain copies of that unrelated
  interface.
- `ExecuteRequest.pty` is implemented by the native sidecar and already used by
  both `openShell` clients. The browser sidecar's typed `unsupported` response is
  an adapter capability, not permission for the SDKs to expose different native
  APIs.
- Raw spawn stdin and stdout/stderr are explicit handle/event methods; there is
  no need for an inert initial-input or host-inheritance option.

### False JavaScript limit overrides

The current config path gives the three unused fields an appearance of support:

- `packages/core/src/agent-os.ts` declares them under
  `AgentOsLimits.jsRuntime`, and `packages/core/src/options-schema.ts` accepts
  them;
- `crates/client/src/config.rs::JsRuntimeLimits` serializes the same three
  values;
- `crates/vm-config/src/lib.rs::JsRuntimeLimitsConfig` accepts them in the
  authoritative create-VM JSON;
- `crates/native-sidecar-core/src/limits.rs::vm_limits_from_config` copies them
  into `JsRuntimeLimits` and `validate_vm_limits` validates them; and
- the legacy config adapter in `crates/sidecar-protocol/src/wire.rs` also
  constructs and detects them.

The chain stops there. Repository-wide field-name searches find no read of
`stdin_buffer_limit_bytes`, `event_payload_limit_bytes`, or
`v8_ipc_max_frame_bytes` outside declarations, parsing, validation, tracing,
and tests. The behavior comes from unrelated constants:

- `crates/execution/src/javascript.rs` defines
  `KERNEL_STDIN_BUFFER_LIMIT_BYTES = 16 MiB` and
  `JAVASCRIPT_EVENT_PAYLOAD_LIMIT_BYTES = 1 MiB`;
- `LocalKernelStdinBridge::write` uses the former directly;
- `send_javascript_event` uses the latter directly; and
- `crates/execution/src/v8_ipc.rs` and
  `crates/v8-runtime/src/ipc_binary.rs` each define their own fixed
  `MAX_FRAME_SIZE = 64 MiB`.

The exact missing handoff is visible at
`crates/native-sidecar/src/execution.rs::javascript_execution_limits`: it copies
heap, sync-RPC, CPU, wall-clock, and import-cache fields into
`JavascriptExecutionLimits`, whose declaration in
`crates/execution/src/javascript.rs` has no stdin-buffer, event-payload, or IPC
frame field. This is executable proof that the resolved values cannot reach the
runtime, not merely a repository-search inference.

The limits audit currently compounds the false claim. Seven entries in
`crates/native-sidecar/tests/fixtures/limits-inventory.json` say these constants
are wired to the dead `VmLimits` fields:

- the three duplicate `DEFAULT_*` constants in
  `crates/native-sidecar-core/src/limits.rs` (delete these entries when the
  constants are deleted);
- `JAVASCRIPT_EVENT_PAYLOAD_LIMIT_BYTES` and
  `KERNEL_STDIN_BUFFER_LIMIT_BYTES` in
  `crates/execution/src/javascript.rs`; and
- both `MAX_FRAME_SIZE` constants in `crates/execution/src/v8_ipc.rs` and
  `crates/v8-runtime/src/ipc_binary.rs`.

Reclassify the four retained runtime constants as `policy-deferred`, remove
their false `wired` values, and say explicitly that they are bounded fixed
runtime policy pending a future cross-adapter tunable design. Do not delete the
real bounds.

This is stronger than an omission bug in one client: both SDKs faithfully
forward values that the trusted runtime ignores. Implementing only the native
stdin field would create a new native/browser divergence, and implementing only
one IPC codec side would break framing. Remove all three unsupported overrides
from the public/config/resolved surfaces in this item. Leave the real constants
where enforcement currently occurs; changing their semantics requires a
separate bounded-transport design, not compatibility plumbing in a client.

## Exact production edits

### TypeScript public types

In `packages/core/src/runtime.ts`:

1. Delete the core `TimingMitigation` type.
2. Delete `filePath`, `cpuTimeLimitMs`, and `timingMitigation` from
   `ExecOptions`.
3. Stop making `KernelSpawnOptions` extend the full `ExecOptions`. Give it only
   the common implemented launch fields (`env`, `cwd`, `timeout`, `onStdout`,
   `onStderr`) plus `streamStdin`.
4. Delete `stdio`, `stdinFd`, `stdoutFd`, `stderrFd`, and public `pty` from
   `KernelSpawnOptions`.
5. Leave `ExecOptions.stdin` and `ExecOptions.captureStdio` intact.
6. Leave every `OpenShellOptions` field intact.

A small non-exported `ProcessLaunchOptions` base may be used inside this file to
avoid repeating the five common fields. Do not introduce a new behavior-owning
class or normalizer.

Recommended final shape:

```ts
interface ProcessLaunchOptions {
	env?: Record<string, string>;
	cwd?: string;
	timeout?: number;
	onStdout?: (data: Uint8Array) => void;
	onStderr?: (data: Uint8Array) => void;
}

export interface ExecOptions extends ProcessLaunchOptions {
	stdin?: string | Uint8Array;
	captureStdio?: boolean;
}

export interface KernelSpawnOptions extends ProcessLaunchOptions {
	streamStdin?: boolean;
}
```

In `packages/core/src/types.ts`, remove `TimingMitigation` from the re-export
list. No explicit root `index.ts` edit is otherwise required because it already
exports `ExecOptions` directly and re-exports `types.ts`.

### TypeScript internal PTY seam

`NativeSidecarKernelProxy` still needs internal `pty`, `shellCommand`, and
`captureOutput` inputs to implement `openShell` and exec over one Execute RPC.
Do not put those implementation knobs back into the public raw-spawn type.

Use a file-private shape:

```ts
interface InternalSpawnOptions extends KernelSpawnOptions {
	pty?: { cols?: number; rows?: number };
	shellCommand?: string;
	captureOutput?: boolean;
}
```

In `packages/core/src/sidecar/rpc-client.ts`:

1. Add a private/internal spawn option type extending the reduced public spawn
   fields with `pty`, `shellCommand`, and `captureOutput`.
2. Move the current spawn body to a private `spawnTracked` helper accepting that
   internal type.
3. Keep the public `spawn` signature on `KernelSpawnOptions` and have it call the
   helper unchanged.
4. Have `exec`, `execArgv`, and `openShell` call the helper for their private
   capture/shell/PTY fields. Pass only `env`, `cwd`, `timeout`, `onStdout`, and
   `onStderr` from exec; do not keep spreading exec-only `stdin` or
   `captureStdio` into the internal spawn bag.
5. Leave `TrackedProcessEntry`, `startTrackedProcess`, and the wire payload
   unchanged.

This preserves one protocol implementation without leaking its internal modes
into a partial public terminal interface. `packages/core/src/agent-os.ts` needs
only the public limit-type deletion described below, not a process-forwarding
behavior change.

### Rust public types and forwarding

In `crates/client/src/process.rs`:

1. Delete `TimingMitigation`.
2. Delete `file_path`, `cpu_time_limit_ms`, and `timing_mitigation` from
   `ExecOptions` and `Default`.
3. Delete `SpawnStdio`.
4. Replace `SpawnOptions.base: ExecOptions` with direct implemented fields:
   `env`, `cwd`, `timeout`, `on_stdout`, `on_stderr`, and `stream_stdin`.
5. Delete `stdio`, `stdin_fd`, `stdout_fd`, and `stderr_fd`.
6. Update `AgentOs::spawn` to take callbacks and forward values from those
   direct fields. Continue sending `capture_output: None` and `pty: None` for raw
   spawn.
7. Keep `ExecOptions.stdin` and `capture_stdio` intact.

The final spawn declaration before Item 46's env-presence follow-up should be:

```rust
#[derive(Default)]
pub struct SpawnOptions {
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub timeout: Option<f64>,
    pub on_stdout: Option<OutputCallback>,
    pub on_stderr: Option<OutputCallback>,
    pub stream_stdin: Option<bool>,
}
```

Take the two callbacks with `options.on_stdout.take()` /
`options.on_stderr.take()`, and forward `options.env`, `options.cwd`,
`options.timeout`, and `options.stream_stdin` directly. Do not add a second
common-options wrapper under another name.

In `crates/client/src/lib.rs`, remove `SpawnStdio` and `TimingMitigation` from
the public process re-exports.

Update the direct Rust client call sites that currently nest
`base: ExecOptions`:

- `crates/client/tests/process_e2e.rs`;
- `crates/client/tests/packages_aospkg_e2e.rs`;
- `crates/client/tests/link_software_e2e.rs`;
- `crates/agentos-actor-plugin/src/actions/process.rs`.

The actor's `ExecActionOptions` and `SpawnActionOptions` contracts do not change.
Only construct the new direct Rust fields in its helper:

```rust
SpawnOptions {
    env: options.env,
    cwd: options.cwd,
    stream_stdin: options.stream_stdin,
    ..SpawnOptions::default()
}
```

### Remove unsupported JavaScript limit overrides

Make the limit removal through the authoritative typed config, then regenerate
the derived declarations:

1. In `crates/vm-config/src/lib.rs`, remove
   `stdin_buffer_limit_bytes`, `event_payload_limit_bytes`, and
   `v8_ipc_max_frame_bytes` from `JsRuntimeLimitsConfig`.
2. In `crates/client/src/config.rs`, remove the corresponding three fields and
   serde attributes from public `JsRuntimeLimits`.
3. In `packages/core/src/agent-os.ts`, remove the three camelCase fields from
   `AgentOsLimits.jsRuntime`; in `packages/core/src/options-schema.ts`, remove
   the three accepted keys.
4. Regenerate `packages/core/src/generated/JsRuntimeLimitsConfig.ts`, then keep
   `packages/runtime-core/src/generated/JsRuntimeLimitsConfig.ts` byte-aligned
   with the same authoritative generated type. Do not hand-preserve deleted
   fields in either copy.
5. In `crates/native-sidecar-core/src/limits.rs`, remove the three duplicate
   default constants, the three fields from resolved `JsRuntimeLimits`, their
   default assignments, override copying, nonzero validation entries, and trace
   snapshot entries. Keep the actual enforcement constants in `execution` and
   `v8-runtime`.
6. In `crates/native-sidecar/src/limits.rs`, remove the three deleted
   compatibility re-exports.
7. In `crates/sidecar-protocol/src/wire.rs`, remove the legacy metadata
   assignments and the three checks in `legacy_has_js_runtime_limits`. Item 45
   later removes this legacy adapter entirely, but Item 43 must keep it compiling
   against the reduced typed config.
8. Update `crates/native-sidecar/tests/limits.rs` and any config literals that
   name the removed fields. Do not change the BARE `ExecuteRequest`: these are
   create-VM JSON fields, not execute-wire fields.
9. Update `crates/native-sidecar/tests/fixtures/limits-inventory.json`: delete
   the three entries for the deleted native-sidecar-core `DEFAULT_*` constants
   and reclassify the four retained execution/V8 constants as
   `policy-deferred` with no `wired` field. The audit must stop claiming the
   removed `VmLimits` fields enforce them.

Do not add these values to `JavascriptExecutionLimits`, browser worker
messages, or V8 binary frames in Item 43. That would expand policy rather than
delete unsupported surface.

### Documentation and generated output

Do not expand Item 43 into the hand-maintained `packages/core/README.md` API
table. That table does not enumerate any field being removed here, and its
incorrect `SpawnOptions` name is already the explicit scope of Item 55. The
durable public surface documentation for this item is the generated `.d.ts`
plus the checked public type fixture. Avoid creating a second field inventory
that can drift.

No BARE schema or generated protocol binding changes are needed. The actor
contract generator should reproduce
`packages/agentos/src/generated/actor-actions.generated.ts` byte-for-byte; build
it and assert no generated diff rather than hand-editing the file.

Do not change:

- `packages/runtime-browser`'s implemented inline-code options;
- `docs/features/typescript.mdx` compiler `filePath` examples;
- website VM resource-limit documentation;
- guest `child_process` fd/stdio implementation;
- sidecar `ExecuteRequest` or PTY behavior.

## Before validation

The current focused baselines pass:

```text
packages/core allowed-node-builtins + spawn-flat-api: 6 tests passed
agentos-client process::tests: 13 tests passed
```

They prove retained paths but do not reject or expose the inert public fields.

### TypeScript public acceptance test

Add `packages/core/tests/process-options.public-api.ts` plus a no-emit
`tsconfig.public-api.json`, and include that project in the package's
`check-types` script. Before production edits, prove the current type surface
accepts:

- exec `filePath`, `cpuTimeLimitMs`, and `timingMitigation`;
- spawn `stdin`, `captureStdio`, the three inherited legacy exec fields,
  `stdio`, all three fd fields, and `pty`.

Use `satisfies ExecOptions` / `satisfies KernelSpawnOptions` object literals so
this is a real TypeScript public API check, not Vitest's transpile-only behavior.
The public config should extend `tsconfig.json`, override `rootDir` to `.`, set
`noEmit: true`, and include `src/**/*` plus this one fixture; do not include the
entire runtime test tree. Chain `tsc --noEmit -p tsconfig.public-api.json` from
the scoped `packages/core` script as:

```json
"check-types": "pnpm run build:protocols && tsc --noEmit && tsc --noEmit -p tsconfig.public-api.json"
```

Record the passing command and parent jj revision in the tracker.

In `packages/core/tests/allowed-node-builtins.test.ts`, add temporary wire
characterization:

- exec with the three legacy exec options emits the same Execute payload as
  omission;
- raw spawn with initial `stdin`, `captureStdio`, `stdio`, and fd overrides emits
  none of those concepts;
- raw spawn with `pty` does emit `ExecuteRequest.pty`, establishing the
  TypeScript/Rust divergence separately from ignored fields.

In the same before fixture, prove `AgentOsOptions` currently accepts all three
unused `limits.jsRuntime` keys. A native integration characterization may set
very small values and demonstrate execution still follows the fixed runtime
caps, but do not retain that test after the keys are removed. The durable
evidence is the public type/schema rejection after removal.

Delete the old-option runtime cases after the API is removed. Retain the
openShell PTY payload test.

### Rust public acceptance test

Add a temporary unit characterization in `crates/client/src/process.rs` that
constructs `ExecOptions` and `SpawnOptions` with every field named above, then
compares the available `ExecuteRequest` builder output with an omitted/default
request. The public structs compile while the wire request has no place for the
values, and raw spawn hard-codes `pty: None`. Record the passing test and parent
revision, then delete this historical characterization with the fields.

Also serialize a temporary `JsRuntimeLimits` containing the three inert values
and assert all three appear in create-VM JSON. Pair it with a temporary
`vm_limits_from_config` characterization showing the values are merely retained
in `VmLimits`; source inspection above proves no execution consumer exists.
Delete this acceptance test with the fields.

Record the current limits-audit result as additional before evidence: it passes
while falsely asserting all seven constants are wired. The after test must pass
with the dead default constants absent and the four actual enforcement constants
truthfully classified as deferred fixed policy.

## After validation

Change the TypeScript public API fixture to prove:

- retained exec and spawn option literals compile;
- each removed field fails independently using one `@ts-expect-error` per
  object literal;
- `OpenShellOptions` still accepts command, args, env, cwd, dimensions, and the
  stderr callback;
- core no longer exports `TimingMitigation`.

Also give each removed `limits.jsRuntime` key its own `@ts-expect-error`, and add
an options-schema test asserting raw JavaScript input containing any one of the
three keys is rejected as unknown. This tests both typed and untyped callers;
unlike arbitrary process option objects, `AgentOsOptions` intentionally has an
existing Zod boundary.

Put each `@ts-expect-error` immediately above its own typed object literal (and
one immediately above the removed type import). This prevents one expected
error from accidentally masking multiple stale fields. The fixture is the
durable after-test; do not rely on a transpile-only Vitest assertion for type
surface removal.

Update `packages/core/tests/public-api-exports.test.ts` to remove its
`TimingMitigation` import/assertion. Keep positive coverage for `ExecOptions`,
`KernelSpawnOptions`, and `OpenShellOptions`.

Replace the temporary Rust historical characterization with a positive unit
test that constructs the reduced `ExecOptions` and flat `SpawnOptions`, feeds
their retained launch values into `build_process_execute_request`, and asserts
`env`, `cwd`, `keep_stdin_open`, `timeout_ms`, and raw-spawn
`capture_output: None` / `pty: None`. Negative Rust compilation machinery is not
warranted solely to memorialize deleted fields; `cargo check`, all updated
struct literals, and the final source-absence assertion enforce their removal.

Strengthen retained-option E2E rather than inventing replacements:

- TypeScript `packages/core/tests/execute.test.ts` already covers exec env, cwd,
  stdin, capture, callbacks, spawn timeout, and spawn streaming stdin. Run it
  with `AGENTOS_E2E_FULL=1` because default Vitest excludes that heavy file.
- `packages/core/tests/spawn-flat-api.test.ts` covers spawned output, explicit
  stdin writes, close, and lifecycle.
- `packages/core/tests/allowed-node-builtins.test.ts` covers presence-aware
  `streamStdin` and internal openShell PTY serialization.
- `packages/core/tests/pty-protocol.test.ts` covers the real terminal interface,
  including dimensions and resize.
- Rust `crates/client/tests/process_e2e.rs` already covers stdin, callbacks,
  capture, timeout, and streaming stdin. Add one env/cwd assertion while updating
  the flat `SpawnOptions` construction.
- `crates/client/tests/shell_pty_packages_e2e.rs` retains command/args/cwd/PTY
  coverage; add env only if another shell E2E does not already prove it.

Run:

```sh
pnpm --dir packages/core check-types
pnpm --dir packages/core build
pnpm --dir packages/core exec vitest run \
	tests/allowed-node-builtins.test.ts \
	tests/spawn-flat-api.test.ts \
	tests/options-schema.test.ts \
	tests/public-api-exports.test.ts
AGENTOS_E2E_FULL=1 pnpm --dir packages/core exec vitest run tests/execute.test.ts
cargo test -p agentos-client --lib process::tests
cargo test -p agentos-client --lib create_vm_config_preserves_typed_limits
cargo test -p agentos-client --test process_e2e
cargo test -p agentos-client --test shell_pty_packages_e2e
cargo test -p agentos-vm-config export_bindings_jsruntimelimitsconfig
cargo test -p agentos-vm-config
cargo test -p agentos-native-sidecar-core limits::tests
cargo test -p agentos-native-sidecar --test limits
cargo test -p agentos-native-sidecar --test limits_audit
cargo check -p agentos-client
cargo test -p agentos-actor-plugin --test action_contract
test -z "$(jj diff -r @-..@ --summary -- \
  packages/agentos/src/generated/actor-actions.generated.ts)"
git diff --check
! rg -n 'file_path|cpu_time_limit_ms|timing_mitigation|SpawnStdio|stdin_fd|stdout_fd|stderr_fd' \
	crates/client/src/process.rs crates/client/src/lib.rs
! rg -n 'stdin_buffer_limit_bytes|event_payload_limit_bytes|v8_ipc_max_frame_bytes' \
	crates/client/src/config.rs crates/vm-config/src/lib.rs \
	crates/native-sidecar-core/src/limits.rs crates/native-sidecar/src/limits.rs \
	crates/sidecar-protocol/src/wire.rs
```

Use a field-aware TypeScript type check rather than a repository-wide search:
related names remain legitimately in `packages/runtime-browser`, compiler
tools, fixed runtime enforcement constants, and guest process code.

After the focused ts-rs export test rewrites
`packages/core/src/generated/JsRuntimeLimitsConfig.ts`, update the identical
runtime-core generated copy with the same one-line type. There is no generator
targeting the runtime-core directory, so verify the two files are byte-equal;
do not hand-preserve the removed fields.

## Proposed small diff sequence

Keep one Item 43 jj revision, but build it as five reviewable sub-diffs:

1. Add and run temporary before characterizations for TS/Rust option
   acceptance, wire omission/divergence, and false limit-config retention.
   Record the parent revision and results in the tracker.
2. Reduce the TypeScript option declarations and introduce only the private
   `spawnTracked` seam needed by `exec`, `execArgv`, and `openShell`; convert the
   temporary public fixture to durable negative/positive type assertions.
3. Reduce Rust `ExecOptions`, flatten only the implemented fields into
   `SpawnOptions`, update its four direct construction sites, and replace the
   temporary characterization with retained-field builder coverage.
4. Delete the three false JavaScript limit overrides end-to-end, regenerate the
   one TS binding, synchronize its runtime-core copy, and correct the seven
   limits-inventory entries. Do not change runtime enforcement constants or
   add protocol fields.
5. Run retained exec/spawn/shell E2Es, schema/type/export tests, the limits
   audit, actor contract generation check, and source-absence assertions; then
   update the tracker and seal the dedicated revision.

## Dependencies, risks, and non-goals

- **Item 42:** stack after its cwd work. Keep `cwd` on every process surface and
  do not undo sidecar-owned relative-path resolution while reshaping options.
- **Item 46:** it later changes Rust `env` from an empty-map sentinel to
  `Option<BTreeMap<...>>`. Apply that change to Item 43's flat `SpawnOptions`;
  never restore `base: ExecOptions`.
- **Item 59:** it assumes public raw-spawn initial `stdin` is gone and uses the
  explicit process stdin route. Do not add an initial-input compatibility shim.
- **Actor generation:** flattening the Rust client struct changes only the
  actor helper's construction site. `SpawnActionOptions` and generated
  TypeScript actor actions remain byte-identical.

- This intentionally breaks Rust struct literals and TypeScript excess-property
  calls that used fields which never worked. The repository has no compatibility
  promise and ships clients with the sidecar in lockstep.
- Refactoring Rust spawn away from `base: ExecOptions` is necessary. Deleting
  only obvious spawn fields would leave ignored exec stdin/capture reachable.
- TypeScript structural typing can allow extra properties stored in a wider
  variable, and untyped JavaScript can pass arbitrary keys. Do not add a
  client-side unknown-key validator solely to police JavaScript objects; that
  would add policy and runtime work to the thin client.
- Keep `captureStdio: false` on exec. It is sidecar-implemented and intentionally
  returns empty captured strings while host callbacks receive raw output.
- Keep all three `streamStdin` states. Item 23 established that omission lets
  the sidecar apply PTY defaults while explicit false and true remain explicit.
- Keep PTY wire support and `openShell`; remove only the incomplete public raw
  spawn option. Browser PTY rejection remains adapter-specific.
- Removing the three false VM-limit overrides is an intentional source/config
  break. Because `JsRuntimeLimitsConfig` denies unknown fields, old raw JSON is
  rejected instead of silently ignored. That is preferable to a false security
  control.
- The fixed JavaScript event limit currently destroys the V8 session without a
  typed limit error reaching the caller. That diagnostics defect is real but is
  not solved by retaining an ignored override. Track it separately if it is not
  already covered by the structured process-error work; do not widen Item 43
  into a V8 event-state-machine rewrite.
- A future configurable stdin/event/IPC limit must define identical native and
  browser semantics, name the limit and how to raise it in typed errors, and
  configure both V8 codec directions. Do not restore only the public fields.
- Host callbacks and process-route maps cannot move into the sidecar because
  they refer to caller closures/subscriptions. Their bounded lifecycle remains
  client-owned.
- Do not add per-execution CPU/timing fields without a separate cross-runtime
  policy design. A generic process may be JavaScript, Python, WASM, or a
  projected command.
- Do not conflate sidecar-host fds with guest kernel fds. Guest fd redirection
  would require an explicit protocol and ownership model, not host integers.

## Dedicated jj revision and path scope

Create exactly one new jj revision on top of the completed Item 42 revision,
without switching the shared working copy to another bookmark. Suggested
description:

```text
refactor(process): remove inert client options
```

Expected path scope:

```text
packages/core/src/runtime.ts
packages/core/src/types.ts
packages/core/src/sidecar/rpc-client.ts
packages/core/src/agent-os.ts
packages/core/src/options-schema.ts
packages/core/src/generated/JsRuntimeLimitsConfig.ts
packages/core/tests/process-options.public-api.ts
packages/core/tsconfig.public-api.json
packages/core/package.json
packages/core/tests/public-api-exports.test.ts
packages/core/tests/options-schema.test.ts
packages/runtime-core/src/generated/JsRuntimeLimitsConfig.ts
crates/client/src/process.rs
crates/client/src/lib.rs
crates/client/src/config.rs
crates/client/tests/process_e2e.rs
crates/client/tests/packages_aospkg_e2e.rs
crates/client/tests/link_software_e2e.rs
crates/agentos-actor-plugin/src/actions/process.rs
crates/vm-config/src/lib.rs
crates/native-sidecar-core/src/limits.rs
crates/native-sidecar/src/limits.rs
crates/native-sidecar/tests/limits.rs
crates/native-sidecar/tests/fixtures/limits-inventory.json
crates/sidecar-protocol/src/wire.rs
docs/thin-client-migration.md
```

`packages/agentos/src/generated/actor-actions.generated.ts` should not change.
There should be no sidecar execution behavior, BARE protocol schema, generated
BARE binding, runtime-browser, or lockfile change. The shared/native limit files
change only to delete the unused resolved fields and compatibility exports. Mark
the before checklist only after old public acceptance/wire/config tests pass on
the parent, mark the after checklist only after the reduced type/schema surface
and retained-option E2Es pass, and mark Item 43 complete only after the dedicated
revision and both revision IDs are recorded.

The temporary old-option cases in `allowed-node-builtins.test.ts` are before
evidence and should leave no final diff. Regular CI already runs scoped
TypeScript typechecks/tests and workspace Rust clippy; the new public type
fixture is therefore durable without a workflow or root-script edit. Focused
native process and shell E2Es remain the implementation gate.
