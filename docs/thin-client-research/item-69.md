# Item 69 research: isolate process-output callbacks from event routing

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 69 tracker status.

## Recommendation

Catch and report each TypeScript stdout/stderr callback independently at both
client fan-out layers, then continue delivering the same event and all later
events. Use one small internal dispatch helper so the public `AgentOs` subscriber
sets and the native proxy use the same behavior.

Delete the proxy's synthetic delivery of an event-pump error to every process's
stderr listeners. A transport failure is not guest stderr and should reject the
affected process waits with its original error after one host-visible pump
diagnostic.

Priority: **P1**. Confidence: **high**.

This behavior must remain in TypeScript. A stdout/stderr callback is an arbitrary
function in the trusted host JavaScript process; the sidecar cannot invoke,
catch, identify, or log that function's exception. The sidecar already does its
only required job here: it emits ordered `process_output` and terminal
`process_exited` events. No Rust, protocol, sidecar, or generated-code change is
needed.

## Original issue and exact failure path

The tracker entries are at `docs/thin-client-migration.md:115,196,282`:

> A TypeScript process output handler can throw through the shared sidecar event
> pump, fail unrelated live process routes, and stop later event delivery.

`NativeSidecarKernelProxy.runEventPump` in
`packages/core/src/sidecar/rpc-client.ts:806-879` waits for one VM-owned event at
a time. Its `process_output` branch invokes listeners without an isolation
boundary:

```ts
const listeners =
	event.payload.channel === "stdout" ? entry.onStdout : entry.onStderr;
for (const listener of listeners) {
	listener(chunk);
}
```

A synchronous listener exception therefore reaches the method's outer `catch`.
That catch assigns the callback exception to `pumpError`, sends its message to
every live route as fabricated stderr, rejects all those process waits, and
returns permanently:

```ts
this.pumpError = error instanceof Error ? error : new Error(String(error));
for (const entry of this.trackedProcesses.values()) {
	const stderr = new TextEncoder().encode(`${this.pumpError.message}\n`);
	for (const listener of entry.onStderr) {
		listener(stderr);
	}
	this.failProcess(entry, this.pumpError);
}
return;
```

The stderr fan-out is itself unguarded. A second throwing stderr handler can
interrupt the failure loop before some entries are settled, leaving the event
pump rejected and those routes live without future delivery.

The scope is precise: each `NativeSidecarKernelProxy` pump is filtered to one
`vmId`, even when several VMs share one native sidecar process. The defect fails
all live process routes in the same proxy/VM. It does not directly stop the
separate proxy pump for another VM.

There is a second fan-out above the proxy. `AgentOs.spawn` in
`packages/core/src/agent-os.ts:1712-1742` gives the proxy one wrapper per channel,
and those wrappers synchronously iterate the public subscriber sets:

```ts
onStdout: (data) => {
	for (const h of stdoutHandlers) h(data);
},
onStderr: (data) => {
	for (const h of stderrHandlers) h(data);
},
```

Even if the proxy catches that wrapper's exception, the first throwing public
subscriber still prevents later subscribers from seeing the same chunk. Both
fan-outs must use per-handler isolation to satisfy the tracker; changing only
`runEventPump` is incomplete.

`NativeSidecarKernelProxy.openShell` has another small wrapper fan-out at
`packages/core/src/sidecar/rpc-client.ts:535-558`. It currently contains one
mutable stdout `onData` callback and at most one initial stderr callback, so it
has no sibling-delivery defect of its own. The lower proxy dispatch boundary
still catches and reports either callback if it throws. Do not add a third
shell-specific mechanism.

## Cross-language, protocol, and generated-surface audit

| Layer | Exact current surface | Item 69 disposition |
|---|---|---|
| Sidecar schema | `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:958-997` defines ordered `ProcessOutputEvent` and `ProcessExitedEvent` payloads. | No edit. A host callback exception is not a guest event and must not be sent over this protocol. |
| Generated TypeScript | `packages/runtime-core/src/generated-protocol.ts:4827-4982` contains the generated process-event codec/union. | No regeneration or hand edit. The wire event remains unchanged. |
| Rust protocol compatibility | `crates/sidecar-protocol/src/protocol.rs:1015-1068,1458-1465` maps the same process payloads without callback semantics. | No edit. |
| Native sidecar | `crates/native-sidecar/src/execution.rs:5468-5596` emits stdout/stderr chunks before the terminal event. | No edit. It cannot invoke or inspect a JavaScript host callback. |
| Browser sidecar | `crates/native-sidecar-browser/src/wire_dispatch.rs:2279-2363` emits the same stdout/stderr and terminal events. | No edit. Existing adapter conformance remains authoritative for event order. |
| Rust SDK | `crates/client/src/process.rs:309-355,1184-1225` gives each `OutputCallback` its own Tokio task and broadcast receiver; one callback panic cannot stop the routing task or sibling callback tasks, and Rust's panic hook is host-visible. Shell callbacks reuse that mechanism at `crates/client/src/shell.rs:121-125`. | No Rust edit or new parity type. This tracker item is the TypeScript shared-call-stack defect. |
| Runtime-core event listeners | `packages/runtime-core/src/protocol-client.ts:367-395` already catches each `onEvent` listener, but currently discards its error. | Item 54 adds its warning. It cannot protect Item 69 because `waitForEvent` has already resolved before the core proxy invokes output callbacks. |

The smallest thin-client-aligned boundary is therefore TypeScript host callback
routing only. The sidecar continues to own process state and output ordering;
the client merely prevents arbitrary trusted-host observer code from corrupting
its transport loop.

## Exact production edits

### 1. Add `packages/core/src/process-output-handlers.ts`

Add one internal helper; do not export it from the package root and do not add a
new client option or policy surface.

```ts
export type ProcessOutputChannel = "stdout" | "stderr";

export interface ProcessOutputHandlerContext {
	channel: ProcessOutputChannel;
	pid?: number;
	processId?: string;
}

function reportProcessOutputHandlerFailure(
	error: unknown,
	context: ProcessOutputHandlerContext,
): void {
	console.error("[agent-os] process output handler failed", {
		error,
		channel: context.channel,
		...(context.pid !== undefined ? { pid: context.pid } : {}),
		...(context.processId !== undefined
			? { processId: context.processId }
			: {}),
	});
}

export function dispatchProcessOutputHandlers(
	handlers: Iterable<(data: Uint8Array) => void>,
	data: Uint8Array,
	context: ProcessOutputHandlerContext,
): void {
	for (const handler of handlers) {
		try {
			handler(data);
		} catch (error) {
			reportProcessOutputHandlerFailure(error, context);
		}
	}
}
```

The diagnostic deliberately excludes the chunk. Process output can be large or
contain caller/guest secrets; channel and route identity are enough to locate
the failed callback. Keep the thrown value unchanged in the structured record
so non-`Error` throws are not silently normalized away.

The declared callback contract is synchronous `void`, so the required Item 69
fix is the `try/catch` above. If rejected async functions are intentionally
accepted later, attach a rejection observer without awaiting it; never stall the
ordered event pump on host callback work. Do not broaden this revision merely
to define asynchronous callback semantics.

### 2. Update `packages/core/src/sidecar/rpc-client.ts`

Import it with:

```ts
import { dispatchProcessOutputHandlers } from "../process-output-handlers.js";
```

In the `process_output` branch of
`NativeSidecarKernelProxy.runEventPump`, replace the raw listener loop with:

```ts
dispatchProcessOutputHandlers(listeners, chunk, {
	channel: event.payload.channel,
	processId: event.payload.process_id,
	...(entry.pid !== null ? { pid: entry.pid } : {}),
});
```

This keeps listener invocation synchronous and in set insertion order while
ensuring one failure cannot be mistaken for a transport failure.

In the outer `catch`, distinguish a genuine wait/transport failure from a host
callback failure (callbacks no longer reach it), log once, fail every live route
with the exact same error object, and remove the synthetic stderr fan-out:

```ts
const pumpError = error instanceof Error ? error : new Error(String(error));
this.pumpError = pumpError;
console.error("[agent-os] sidecar process event pump failed", {
	error: pumpError,
	vmId,
});
for (const entry of [...this.trackedProcesses.values()]) {
	if (!entry.settled) {
		this.failProcess(entry, pumpError);
	}
}
return;
```

Use `entry.settled`, not `entry.exitCode !== null`: a failed entry can be settled
while retaining a null exit code. Snapshotting the map is defensive because
`failProcess` deletes entries from both tracking maps.

Do not rethrow, terminate a process, synthesize an exit code, inject the callback
message into guest stderr, or send a callback-result RPC. A failed observer does
not change process state. A genuine pump failure already reaches callers through
the rejected `ManagedProcess.wait()`/exec promise.

### 3. Update `packages/core/src/agent-os.ts`

Import the same helper with:

```ts
import { dispatchProcessOutputHandlers } from "./process-output-handlers.js";
```

In `AgentOs.spawn`, declare
`let processPid: number | undefined` immediately before calling
`this.#kernel.spawn`, replace both raw public-handler loops, then assign
`processPid = proc.pid` after the awaited spawn returns:

```ts
let processPid: number | undefined;
const proc = await this.#kernel.spawn(command, args, {
	...options,
	onStdout: (data) => {
		dispatchProcessOutputHandlers(stdoutHandlers, data, {
			channel: "stdout",
			...(processPid !== undefined ? { pid: processPid } : {}),
		});
	},
	onStderr: (data) => {
		dispatchProcessOutputHandlers(stderrHandlers, data, {
			channel: "stderr",
			...(processPid !== undefined ? { pid: processPid } : {}),
		});
	},
});
processPid = proc.pid;
```

This is the isolation point that lets a later `onProcessStdout` or
`onProcessStderr` subscriber receive the same chunk after an earlier subscriber
throws. PID is optional because a nonstandard kernel could synchronously emit
output before its spawn promise resolves; output isolation must not depend on
that correlation being available. The lower proxy helper remains necessary for
direct kernel `exec`, `execArgv`, `openShell`, and other internal callers that do
not pass through these public sets.

Do not delete a throwing subscriber automatically. JavaScript event APIs do not
normally unsubscribe a listener because it threw, and automatic removal would
add client-owned retry/lifecycle policy. A repeatedly failing callback produces
one diagnostic per invocation until its owner calls the existing unsubscribe
function.

### 4. Update the public process documentation

After the process `<CodeSnippet>` in
`website/src/content/docs/docs/core.mdx:52-56`, add one behavioral sentence:

```mdx
Process output callbacks are isolated observers. If one throws, AgentOS reports
the host callback failure and continues delivering ordered output to the other
listeners without changing the guest process.
```

This is a public callback-contract change. It needs documentation, but no new
option, exported type, or runnable example.

## Focused before/after tests

### Low-level pump and sibling-process regression

Extend `packages/core/tests/process-event-ordering.test.ts` because it already
drives the real `NativeSidecarKernelProxy.runEventPump` with a deterministic
queued client.

First make `createStubClient().execute()` return unique process IDs/PIDs while
preserving the existing first values:

```ts
let processSequence = 0;
async execute() {
	processSequence += 1;
	return {
		processId: `process-${processSequence}`,
		pid: 4241 + processSequence,
	};
}
```

Also give the deterministic client an explicit pump-rejection seam rather than
reaching into proxy internals. Change the queue to a result union and have the
existing `deliver` closure resolve or reject the pending wait:

```ts
type PumpResult = { event: PumpEvent } | { error: Error };
const queue: PumpResult[] = [];

// Inside deliver():
const result = queue.shift();
if (!result) return false;
if ("error" in result) reject(result.error);
else resolve(result.event);
return true;

// Returned test controls:
pushEvent(event: PumpEvent) {
	queue.push({ event });
	notify?.();
},
failPump(error: Error) {
	queue.push({ error });
	notify?.();
},
```

Keep the existing `notify` reset after any delivered result, and make the abort
listener `{ once: true }`. This makes the real-pump-failure case deterministic
without exposing a test-only production method.

Add a table-driven test for both `stdout` and `stderr`:

1. spawn two processes on the same proxy;
2. give process 1 a channel callback that throws `handlerFailure` only on its
   first invocation;
3. attach resolved expectations to both `wait()` promises before emitting;
4. push process 1 output, later process 1 output, process 2 output, and ordered
   terminal events for both;
5. assert both waits resolve with their own exit codes, process 1 receives its
   later event, and process 2 receives its exact output in order; and
6. spy on `console.error` and assert exactly one
   `"[agent-os] process output handler failed"` record containing the original
   thrown object, channel, `pid: 4242`, and `processId: "process-1"`.

Against current code this test proves the **before** behavior: both process waits
reject with `handlerFailure`, later queued output is starved, and no structured
callback diagnostic is emitted. With the fix it proves the **after** behavior
and confirms the pump remains live.

Add a separate real-pump-failure case to the same file: make the stub's next
`waitForEvent` reject with a distinctive transport error while two processes
are live. Assert one structured pump diagnostic, both waits reject with the
same error object, and neither stderr callback receives fabricated bytes. This
locks in removal of the non-Linux synthetic stderr behavior.

### Public sibling-handler regression

Extend `packages/core/tests/leak-agent-os-processes.test.ts`, reusing its minimal
private-constructor `AgentOs` fixture. Supply a mock kernel whose `spawn` captures
the forwarded `onStdout`/`onStderr` wrapper and returns a controllable
`ManagedProcess`. For each channel:

1. pass a throwing callback in `vm.spawn(..., { onStdout/onStderr })`;
2. add a second callback with `vm.onProcessStdout`/`onProcessStderr`;
3. invoke the captured kernel callback with one exact `Uint8Array`;
4. assert invocation does not throw, the second callback receives the same
   buffer object, and the structured error contains the original exception and
   channel; and
5. unsubscribe/settle the mock process so cleanup remains deterministic.

This fails before the `AgentOs.spawn` edit because the first callback throws
through the wrapper and the second callback is skipped. It prevents an
implementation that fixes only the low-level pump but still violates the
public subscriber contract.

The exact fixture change is to add an optional kernel override without altering
the existing seven private-constructor arguments:

```ts
function makeAgentOs(
	processRouteRetention = 1_024,
	kernelOverrides: Record<string, unknown> = {},
) {
	const kernelMock = {
		dispose: async () => {},
		...kernelOverrides,
	};
	// Existing constructor setup remains unchanged.
}
```

For the new test, use `makeProc` so `wait()` stays pending while the captured
output wrapper is invoked. The kernel override's `spawn` stores the forwarded
options and returns that process. Resolve the process afterward, await its
terminal correlation, restore the console spy, and call `vm.dispose()` in
`finally`. Assert the structured diagnostic includes `pid` once `vm.spawn` has
returned.

No test moves to Rust or the sidecar. Neither environment can create a throwing
host JavaScript callback. Existing sidecar process-event ordering tests remain
the authority for output-before-exit wire behavior.

## Dependencies, risks, and non-goals

- **Item 54 is separate:** its `SidecarProtocolClient.dispatchEvent` listener
  warning protects direct transport subscribers. Item 69 throws after
  `waitForEvent` has resolved, inside the core proxy, so Item 54 cannot fix it.
- **Item 63 touches the neighboring terminal branch:** stack Item 69 after Item
  63 or resolve the small import/hunk overlap without changing
  `ProcessTerminalError` behavior.
- **Item 59 owns finite-input tests in the same fixture:** it may change the
  first three `process-event-ordering.test.ts` cases or the stub request surface
  before Item 69 lands. Rebase the queue/unique-ID changes onto its final helper;
  do not restore removed client-side stdin sequencing.
- **Items 57 and 70 may touch process-route tests/state:** preserve their
  terminal-route and cache changes. Item 69 owns callback dispatch only.
- **Mutation during dispatch:** JavaScript `Set` iteration has live mutation
  semantics. Keep the current direct iteration; do not snapshot or redefine
  subscribe/unsubscribe-during-callback behavior in this item.
- **Output ordering:** do not defer callbacks with `queueMicrotask`, promises,
  or a side queue. Catching synchronously preserves sidecar event order.
- **No callback-to-sidecar protocol:** reporting an exception to the sidecar
  would falsely make a host observer part of process execution and could let a
  UI/listener failure affect guest state.
- **No new public error class:** Item 63 owns exported operation errors. A small
  structured log record is sufficient here and avoids expanding the client API.
- **Item 74 owns post-failure starts:** after a genuine pump failure,
  `startTrackedProcess` does not currently consult `pumpError`, so a later spawn
  can start without a live event consumer. Item 69 must preserve `pumpError` and
  exact rejection identity for Item 74, but must not add its preflight/race
  state machine here.

## Dedicated JJ revision and bounded paths

Implement Item 69 in one dedicated child revision. The expected diff is bounded
to:

```text
packages/core/src/process-output-handlers.ts
packages/core/src/sidecar/rpc-client.ts
packages/core/src/agent-os.ts
packages/core/tests/process-event-ordering.test.ts
packages/core/tests/leak-agent-os-processes.test.ts
website/src/content/docs/docs/core.mdx
docs/thin-client-migration.md       # checklist/status only after validation
```

No Rust client, sidecar crate, protocol schema, generated codec, package root
export, or public API type should change.

Suggested description:

```text
fix(client): isolate process output handlers
```

## Validation commands

Record the focused tests failing before the production edit, then passing after:

```sh
pnpm --dir packages/core exec vitest run tests/process-event-ordering.test.ts tests/leak-agent-os-processes.test.ts --reporter=verbose
pnpm --dir packages/core check-types
pnpm --dir website build
git diff --check
```

The current pre-edit focused baseline is **11/11 passing** across the two test
files. Add the five desired cases first (stdout/stderr low-level isolation, one
genuine pump failure, and stdout/stderr public sibling isolation), run them
against the old production code, and save their failures as the tracker's
before evidence. Then apply the production edits and rerun the complete command
set above for after evidence.

Item 69 is complete only when stdout and stderr failures are independently
host-visible, later subscribers receive the same chunk, unrelated processes on
the proxy continue receiving ordered output/exit events, genuine pump failures
do not masquerade as guest stderr, the tracker contains both before/after test
evidence, and the implementation is isolated in its dedicated stacked revision.
