# Item 63 implementation research: preserve TypeScript operation-error context

Status: implementation-ahead research only. This note changes no production
code, tests, protocol, or tracker status.

## Decision

Add two exported TypeScript error classes:

- `ProcessTerminalError`, with the exact sidecar code/message and the complete
  terminal `LiveEventFrame`; and
- `AcpResponseError`, with the exact adapter code/message and the complete ACP
  extension envelope returned by the sidecar.

Replace only the two anonymous `Error & { code }` constructions. Preserve the
received source objects by identity and do not parse, normalize, remap, retry,
or otherwise interpret either code.

Priority: **P2**. Fix confidence: **high**.

This is thin-client-compliant host diagnostics. The sidecar and native/shared
ACP adapter already own the failure semantics. A Rust sidecar cannot construct
an ergonomic JavaScript `Error` subclass; only the TypeScript API boundary can
expose received wire context through `instanceof` and public properties.

## Tracker anchors and executable before state

Item 63 is currently recorded at:

- `docs/thin-client-migration.md:109` -- issue and recommended fix;
- `docs/thin-client-migration.md:190` -- `pending`, P2/high confidence; and
- `docs/thin-client-migration.md:276` -- before/after/dedicated-revision checks.

The current focused baseline is green:

```sh
pnpm --dir packages/core exec vitest run \
  tests/process-event-ordering.test.ts \
  tests/session-route-registration.test.ts \
  tests/public-api-exports.test.ts --reporter=verbose
```

It reports 15 passing tests, but none asserts an exported terminal/ACP error
class or retained source frame. The new assertions should first be added and run
against current production code; they will fail because both values are plain
`Error` objects and neither has `event`/`envelope` context. Record that red run
in the tracker before implementing the classes.

Existing real-sidecar coverage at `packages/core/tests/execute.test.ts:96-109`
proves only that a process terminal failure has anonymous `.code` and `.message`
properties. It is useful before-behavior evidence and should be upgraded to
assert the new public class after the focused test is added.

## Exact current TypeScript paths

### Process terminal event

`NativeSidecarKernelProxy.runEventPump` at
`packages/core/src/sidecar/rpc-client.ts:806-881` receives the complete event
from `waitForEvent`. The `process_exited` error branch at lines 835-846 currently
does this:

```ts
const error = new Error(
	event.payload.error_message ?? event.payload.error_code,
) as Error & { code: string };
error.code = event.payload.error_code;
this.failProcess(entry, error);
```

That retains only message/code and discards direct access to the source frame's
ownership, process ID, exit code, captured stdout/stderr, schema, and their
correlation.

The source is already complete:

- `packages/runtime-core/src/protocol-frames.ts:48-53` defines
  `LiveEventFrame`;
- `packages/runtime-core/src/event-buffer.ts:21-53` defines the live terminal
  payload, including optional `error_code`/`error_message`;
- `event-buffer.ts:364-415`, especially lines 380-397, converts the generated
  BARE event without losing terminal fields; and
- `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:969-975` already
  carries `processId`, `exitCode`, optional captured output, and one complete
  `RejectedResponse`.

`failProcess` at `rpc-client.ts:901-908` rejects the same tracked wait promise.
`AgentOs._trackProcess` at `packages/core/src/agent-os.ts:1647-1705` preserves
that exact `Error` object in a compact failed route, and the process methods at
lines 1742-1812 rethrow/reject it without reconstruction. The new class will
therefore reach finite `exec`/`execArgv`, `ManagedProcess.wait()`, and retained
`AgentOs.waitProcess` paths automatically.

### ACP response

`AgentOs._decodeAcpResponseEnvelope` at
`packages/core/src/agent-os.ts:2543-2559` receives both the original extension
envelope and decoded response. Its `AcpErrorResponse` branch at lines 2551-2556
currently does this:

```ts
const error = new Error(response.val.message) as Error & {
	code?: string;
};
error.code = response.val.code;
throw error;
```

The code/message survives, but the source `{ namespace, payload }` does not.
The live envelope is already defined at
`packages/runtime-core/src/ext.ts:4-7`. The generated response variant is at
`packages/core/src/sidecar/agentos-protocol.ts:908-923,959-969`, backed by
`crates/agentos-protocol/protocol/agent_os_acp_v1.bare:198-201,217-227`.

`_sendAcpRequest` at `agent-os.ts:2561-2589` runs this same decoder for both the
optional response hook and the ordinary returned envelope. Replacing the one
throw site covers create/resume/session operations without changing routing.

### Existing structured-error precedent

`SidecarRequestRejected` at
`packages/runtime-core/src/sidecar-errors.ts:8-26` is the right model: an
exported `Error` subclass preserving exact code/message and its complete source
response frame. Core re-exports it at `packages/core/src/index.ts:43`.

Do not broaden `KernelError` (`packages/core/src/memory-filesystem.ts:12-20`):
it is a filesystem/kernel errno class, prefixes some messages, and has no source
protocol object. Do not reuse `SidecarRequestRejected`: process terminal events
and decoded ACP extension errors are not ordinary request rejection frames.

## Sidecar/runtime and Rust audit

No protocol, runtime conversion, sidecar, or Rust production edit is required.

### Process producer

- Native sidecar terminal construction is at
  `crates/native-sidecar/src/execution.rs:5552-5594`; it forwards the capture
  error into `ProcessExitedEvent.error`.
- Browser-sidecar tests at
  `crates/native-sidecar-browser/tests/wire_dispatch.rs:1660-1705` and
  `:1872-1906` prove the same stable capture-limit error crosses its terminal
  event.
- Runtime-core conversion coverage at
  `packages/runtime-core/tests/event-buffer.test.ts:195-217` proves exact
  code/message/output conversion into the live event.

The JavaScript client is the only layer currently dropping direct source-frame
access.

### ACP producer

The shared ACP core builds the authoritative wire error at
`crates/agentos-sidecar-core/src/lib.rs:95-100`; the BARE envelope already
contains its exact code/message. Native and browser adapters both use the shared
response. Item 63 must not add another client code taxonomy.

### Rust client parity

Rust already exposes a public, downcastable enum instead of anonymous errors:

- `crates/client/src/error.rs:14-38` defines `ClientError::Kernel { code,
  message }`;
- `crates/client/src/process.rs:966-1000` maps terminal event errors to that
  typed variant; and
- `crates/client/src/session.rs:1592-1621` maps decoded ACP errors to the same
  typed variant without changing code/message.

Rust does not retain a full event/envelope in that variant, but its caller can
already reliably identify the error and recover the authoritative fields. Item
63 explicitly tracks the TypeScript anonymous-error defect. Adding Rust wire
frame copies or new enum variants would expand the item and change a stable
public taxonomy without fixing a current loss of code/message.

Behavioral parity remains: both clients return the same authoritative
code/message and neither interprets policy. TypeScript additionally follows its
existing `SidecarRequestRejected.response` convention by retaining the
JavaScript source object it already received.

## Smallest production edit

### 1. Add `packages/core/src/operation-errors.ts`

Use only type imports from the already exported runtime-core subpaths:

```ts
import type { LiveExtEnvelope } from
	"@rivet-dev/agentos-runtime-core/ext";
import type { LiveEventFrame } from
	"@rivet-dev/agentos-runtime-core/protocol-frames";

export type ProcessTerminalErrorEvent = LiveEventFrame & {
	payload: Extract<
		LiveEventFrame["payload"],
		{ type: "process_exited" }
	> & { error_code: string };
};

/** A sidecar-reported guest-process terminal failure with its source event. */
export class ProcessTerminalError extends Error {
	readonly code: string;
	readonly event: ProcessTerminalErrorEvent;

	constructor(event: ProcessTerminalErrorEvent) {
		super(event.payload.error_message ?? event.payload.error_code);
		this.name = "ProcessTerminalError";
		this.code = event.payload.error_code;
		this.event = event;
	}
}

/** A decoded ACP adapter rejection with its original extension envelope. */
export class AcpResponseError extends Error {
	readonly code: string;
	readonly envelope: LiveExtEnvelope;

	constructor(options: {
		code: string;
		message: string;
		envelope: LiveExtEnvelope;
	}) {
		super(options.message);
		this.name = "AcpResponseError";
		this.code = options.code;
		this.envelope = options.envelope;
	}
}
```

This is smaller and cleaner than publicly exposing the generated tagged ACP
union. The envelope is the requested complete source; callers already get the
decoded authoritative code/message as first-class properties. Do not add a
second decoded-response copy, code enum, retryability flag, message prefix,
errno conversion, or base class.

Keep object identity rather than copying/freeze-cloning the frame or
`Uint8Array`. That matches `SidecarRequestRejected.response`, avoids client-owned
work, and lets tests prove the actual received wire object is inspectable.

### 2. Edit `packages/core/src/sidecar/rpc-client.ts`

Import `ProcessTerminalError` from `../operation-errors.js`. Replace only lines
840-845 with:

```ts
if (event.payload.error_code !== undefined) {
	this.failProcess(
		entry,
		new ProcessTerminalError(event as ProcessTerminalErrorEvent),
	);
	continue;
}
```

Import `ProcessTerminalErrorEvent` as a type alongside the value. TypeScript
narrows the nested `event.payload`, but not the containing `event` object, so one
assertion is needed at this already checked protocol boundary. Do not rebuild a
partial frame to avoid it.

Do not change the event pump, process route lifecycle, output capture, callback
order, or error identity after `failProcess`.

### 3. Edit `packages/core/src/agent-os.ts`

Import `AcpResponseError` from `./operation-errors.js`. Replace only the
`AcpErrorResponse` body at lines 2551-2556:

```ts
if (response.tag === "AcpErrorResponse") {
	throw new AcpResponseError({
		code: response.val.code,
		message: response.val.message,
		envelope,
	});
}
```

Do not move BARE decoding into the class. Wrong namespace and decode failures
remain malformed transport/input errors, not adapter-owned operation
rejections.

### 4. Edit `packages/core/src/index.ts`

Add root exports next to the existing public error exports:

```ts
export {
	AcpResponseError,
	ProcessTerminalError,
} from "./operation-errors.js";
export type { ProcessTerminalErrorEvent } from "./operation-errors.js";
```

No actor-package re-export is needed. These errors are thrown by direct
`@rivet-dev/agentos-core` VM/session calls; actor actions cross a serialized
boundary and do not expose these core objects by identity.

### 5. Document the public behavior

Add a compact “Operation errors” subsection to
`website/src/content/docs/docs/core.mdx` after the direct process/session API
sections. State that `ProcessTerminalError` retains `.code` and `.event`, while
`AcpResponseError` retains `.code` and `.envelope`, and that these values are the
unmodified sidecar/adapter result. No runnable example is needed.

The class JSDoc is still required because the website TypeDoc entry point is the
actor package rather than the core package; the public core declarations are the
authoritative API documentation for direct consumers.

## Exact red/green tests

### Process terminal: `packages/core/tests/process-event-ordering.test.ts`

At lines 7-58, replace the loose `PumpEvent` fixture with full live frames while
keeping existing call sites compact:

```ts
import type { LiveEventFrame } from
	"@rivet-dev/agentos-runtime-core/protocol-frames";
import { SIDECAR_PROTOCOL_SCHEMA } from
	"@rivet-dev/agentos-runtime-core/protocol-schema";
import {
	ProcessTerminalError,
} from "../src/index.js";

// queue and waitForEvent use LiveEventFrame
pushEvent(event: Omit<LiveEventFrame, "frame_type" | "schema">) {
	const frame: LiveEventFrame = {
		frame_type: "event",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		...event,
	};
	queue.push(frame);
	notify?.();
	return frame;
}
```

Add a test that spawns, starts `proc.wait()`, and pushes one terminal event with
distinct ownership, process ID, exit code, stdout, stderr, `error_code`, and
`error_message`. Capture the rejection once and assert:

- `error instanceof ProcessTerminalError`;
- exact unmodified `name`, `code`, and `message`;
- `error.event === sourceEvent`;
- representative ownership/process/exit fields; and
- the same stdout/stderr byte objects remain reachable through the event.

Before implementation, `ProcessTerminalError` is not exported and the plain
error has no `.event`. After implementation, the same test proves the full
received frame survives the route.

### Real process integration: `packages/core/tests/execute.test.ts`

At lines 96-109, import `ProcessTerminalError`, capture the real sidecar
rejection once, and add:

```ts
expect(error).toBeInstanceOf(ProcessTerminalError);
expect(error).toMatchObject({
	code: "ERR_CAPTURED_OUTPUT_LIMIT_EXCEEDED",
	message: expect.stringContaining(
		"limits.jsRuntime.capturedOutputLimitBytes",
	),
});
expect(error.event.payload).toMatchObject({
	type: "process_exited",
	error_code: "ERR_CAPTURED_OUTPUT_LIMIT_EXCEEDED",
});
```

This keeps the existing functionality assertion and proves a real native
sidecar event reaches the new class. The cheap ordering test remains the exact
identity test.

### ACP error: `packages/core/tests/session-route-registration.test.ts`

Add a standalone injected-agent test using the public root export. Its fake
`extensionRequest` should intentionally ignore the optional response hook and
return this exact object:

```ts
const sourceEnvelope = {
	namespace: ACP_EXTENSION_NAMESPACE,
	payload: encodeAcpResponse({
		tag: "AcpErrorResponse",
		val: {
			code: "adapter_rejected",
			message: "adapter said no",
		},
	}),
};
```

Call `agent.createSession("test-agent")`, capture the rejection once, and
assert:

- `error instanceof AcpResponseError`;
- exact `name`, `code`, and message;
- `error.envelope === sourceEnvelope` and the payload bytes are the same object;
- no session route was created.

This exercises `_sendAcpRequest` and the public create path without calling the
private decoder directly. Current code fails only the class/context assertions;
the final implementation must not create a route before rejecting.

### Root contract: `packages/core/tests/public-api-exports.test.ts`

Import both classes and `ProcessTerminalErrorEvent` from `../src/index.js`.
Assert both values are functions and reference the event type in the existing
type-export test. This prevents an internal-only implementation consumers
cannot identify with `instanceof`.

No Zod, sidecar, protocol, generated-code, or Rust test should move. This item
changes JavaScript error presentation after authoritative data already arrived;
the existing sidecar/runtime tests remain the owner of wire production and
conversion.

## Dependencies, overlap, and risks

- **Item 26 is the precedent:** it introduced
  `SidecarRequestRejected.response`. Reuse its preservation model rather than
  creating another policy layer.
- **Item 57 must preserve identity:** its result-bearing process-exit callback
  should forward the exact `ProcessTerminalError`, not wrap or stringify it.
- **Item 64 is independent:** cron must pass through
  `SidecarRequestRejected`; it should not reuse either Item 63 class.
- **Item 65 benefits automatically:** aggregate cleanup must retain these class
  instances unchanged if they become child errors.
- **Item 69 overlaps `runEventPump`:** it changes output-listener isolation in a
  neighboring branch. Stack/rebase carefully and do not combine revisions.
- **Item 70/72 process retention:** compact failed routes must continue retaining
  the exact typed error for late waits/subscribers.
- **Item 75 depends on code preservation:** a shared `session_not_found` ACP
  response should surface as `AcpResponseError.code` without a TypeScript
  missing-session lookup or message parser.
- **Hook path:** throwing during `_decodeAcpResponseEnvelope` inside an optional
  response hook must remain a rejection of the original request; do not catch
  and reconstruct it later.
- **Source mutation:** frame/envelope and byte arrays are retained references.
  This matches existing error behavior and avoids an unnecessary clone. Document
  them as diagnostic source data, not mutable client policy state.
- **Malformed envelopes remain separate:** unexpected namespace and codec
  errors are not authoritative ACP operation responses and must remain ordinary
  transport/decode failures.
- **No new limits/defaults:** the classes add no queue, cache, retry, timeout, or
  policy.

## Dedicated JJ revision and bounded diff

Implement Item 63 in one new child `jj` revision stacked on the preceding item,
as required by `docs/thin-client-migration.md:25-31`. Expected paths:

```text
packages/core/src/operation-errors.ts
packages/core/src/sidecar/rpc-client.ts
packages/core/src/agent-os.ts
packages/core/src/index.ts
packages/core/tests/process-event-ordering.test.ts
packages/core/tests/execute.test.ts
packages/core/tests/session-route-registration.test.ts
packages/core/tests/public-api-exports.test.ts
website/src/content/docs/docs/core.mdx
docs/thin-client-migration.md       # evidence/status only after validation
```

No Rust SDK, protocol schema, generated codec, runtime-core implementation,
sidecar, actor, package dependency, or registry file should change.

Suggested revision description:

```text
fix(core): preserve operation error protocol context
```

## Before/after validation commands

Focused cheap tests:

```sh
pnpm --dir packages/core exec vitest run \
  tests/process-event-ordering.test.ts \
  tests/session-route-registration.test.ts \
  tests/public-api-exports.test.ts --reporter=verbose
```

Real native-sidecar regression and package gates:

```sh
pnpm --dir packages/core exec vitest run tests/execute.test.ts --reporter=verbose
pnpm --dir packages/core check-types
pnpm --dir packages/core build
pnpm --dir website build
git diff --check
```

Item 63 is complete only when the pre-change focused assertions record the two
anonymous/contextless failures, both public classes retain exact unmodified
code/message and source object identity, real process integration passes, the
root export contract and core docs are current, malformed-envelope behavior is
unchanged, all focused gates pass, and the tracker checklist/status are marked
done in the dedicated revision.
