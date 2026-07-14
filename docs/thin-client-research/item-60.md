# Item 60 research: make shell stdin forwarding fail closed

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 60 tracker status.

Inspected and revalidated on **2026-07-14** through revision **`16a41860`**.
Tracker anchors are `docs/thin-client-migration.md:106` (issue inventory),
current line 193 (pending status), and current line 285
(before/after/complete checklist).

## Recommendation

Replace both ad hoc `stdinQueue = stdinQueue.then(...)` loops in the shell CLI
with one small host-only serialized-input forwarder. The forwarder should:

1. serialize data and EOF operations in arrival order;
2. make the first rejected operation terminal;
3. stop accepting later input;
4. invoke one supplied abort operation exactly once; and
5. expose an immediate failure signal plus a final rejection after abort, so the
   command runner cannot mistake the exit caused by its own abort for success.

Use `killProcess(pid)` as the spawned-process abort and `closeShell(shellId)` as
the PTY abort. Preserve normal EOF behavior: `closeProcessStdin(pid)` for a
spawned process and the existing `"\u0004"` terminal input for a PTY. If abort
also fails, reject with `AggregateError([inputError, abortError])` so neither
failure is discarded.

This is legitimate client-side functionality: only the Node CLI can observe its
host `process.stdin` stream. It should contain no VM policy or emulation. The
sidecar remains authoritative for stdin delivery, EOF semantics, process IDs,
signals, terminal lifecycle, and exit status.

Priority: **P1**. Confidence: **high**. The failure is a direct JavaScript
promise-chain consequence, both affected loops are local, and all required
sidecar termination operations already exist on both direct and actor backends.

## Cross-layer disposition

| Layer | Exact current code | Item 60 disposition |
|---|---|---|
| Shell CLI | `packages/shell/src/main.ts:506-560` and `:588-686` | **Change.** Replace both poisoned promise chains, distinguish host error from EOF, fail the command visibly, and invoke authoritative abort once. |
| Shared direct/actor handle | `packages/shell/src/actor-vm.ts:43-72` and adapter object at `:372-458` | **Change.** Expose and forward the existing `killProcess` and `closeShell` operations. Do not implement signal or terminal policy here. |
| TypeScript product/runtime | `packages/core/src/agent-os.ts:1743-1758,1964-1968,2017-2033,2123-2131`; wire calls in `packages/runtime-core/src/sidecar-process.ts:1574-1622` | **No behavioral change.** These already forward write, close-stdin, and kill requests and retain sidecar-authoritative lifecycle state. |
| Protocol | `WriteStdinRequest`, `CloseStdinRequest`, and `KillProcessRequest` in `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:389-407`, union entries at `:517-520`; TypeScript live/conversion types at `packages/runtime-core/src/request-payloads.ts:191-210,508-534` | **No change.** The required streaming and abort messages already exist. Adding an Item 60 protocol queue would duplicate a host event-loop concern. |
| Native/browser sidecar | dispatch at `crates/native-sidecar/src/service.rs:3936-3977`, execution at `crates/native-sidecar/src/execution.rs:4209-4300`, and browser dispatch at `crates/native-sidecar-browser/src/wire_dispatch.rs:2099-2180` | **No change.** Continue owning byte delivery, EOF, process/PTY termination, and authoritative exits. |
| Rust SDK/actor adapter | process/shell client operations in `crates/client/src/process.rs:672-810` and `crates/client/src/shell.rs:295-460`; existing actor forwarding in `crates/agentos-actor-plugin/src/actions/process.rs:87-147` and `actions/shell.rs:332-357` | **No Rust SDK change.** Rust has no Node stdin callback chain. The actor methods already exist; only the TypeScript shell wrapper omits them. |
| Docs/tests | tracker rows above; shell smoke tests in `packages/shell/tests/cli.test.ts:67-105`; lifecycle tests in `packages/core/tests/execute.test.ts:49-55`, `crates/native-sidecar/tests/kill_cleanup.rs`, and `crates/client/tests/shell_e2e.rs:119-120` | **Add focused shell tests and tracker evidence only.** Retain existing sidecar lifecycle coverage rather than moving a host promise-queue regression there. |

This is the narrow thin-client exception allowed by the boundary: the CLI must
observe and serialize its own host `process.stdin` callbacks because the
sidecar cannot see callbacks that Node never forwarded. The helper must not
invent buffering, retry, EOF, signal, or process policy.

## Original issue and exact failure

### Spawned-process path

`runSpawnedCommand` in `packages/shell/src/main.ts:506-560` currently creates a
mutable promise at lines 522-529:

```ts
let stdinQueue = Promise.resolve();
const queueStdin = (operation: () => Promise<void>) => {
	stdinQueue = stdinQueue.then(operation);
	void stdinQueue.catch((error) => {
		process.stderr.write(`${message}\n`);
	});
};
```

The data listener queues `writeProcessStdin` at lines 539-541. The EOF/error
listener queues `closeProcessStdin` at lines 530-537. If a write rejects, the
queue remains rejected. A later call performs
`rejectedPromise.then(closeOperation)`, so the close operation is never called.
The attached `catch` only prints the error; it does not repair the chain, reject
`runSpawnedCommand`, or terminate the child. Line 553 continues awaiting
`waitProcess`, which can remain pending because the child is still waiting for
EOF.

There is a second failure in the same path: `closeChildStdin` catches and
silently discards every close error at lines 531-536. The comment says the
process may already have exited, but the code does not establish that. If the
process is still alive and the close request fails, both interactive and
`--no-interactive` execution can wait forever. The replacement must remove this
blanket catch; a real close failure is terminal unless the process wait has
already authoritatively completed.

Concrete event sequence:

```text
stdin data A -> queue write(A)
stdin end    -> queue closeStdin after write(A)
write(A) rejects
closeStdin is skipped because its parent promise is rejected
the rejection is printed, but waitProcess never settles
```

### Terminal/PTY path

`runTerminalAttempt` in `packages/shell/src/main.ts:588-686` repeats the same
pattern at lines 622-629. Normal data queues `writeShell`; EOF/error queues an
EOT byte (`"\u0004"`) through that same poisoned chain at lines 630-635. A
rejected earlier write therefore prevents EOT from reaching the terminal, and a
rejected EOT is only logged. Line 667 can wait on `waitShell` forever.

The cleanup operation already exists. `AgentOs.closeShell` kills the
sidecar-owned shell at `packages/core/src/agent-os.ts:2017-2033`, and the actor
action forwards the same operation at
`crates/agentos-actor-plugin/src/actions/shell.rs:350-353`. The shell's common
`ShellVmHandle` simply does not expose it today.

While tracing EOF, one adjacent Linux-behavior gap is visible: when terminal
mode is selected with `--no-interactive`, lines 651-661 attach no stdin listener
but also send no initial EOT. Docker-style `-t` without `-i` has closed stdin;
the replacement should call the forwarder's `end()` immediately in this branch
instead of leaving an interactive command waiting on an input stream the CLI
will never drive.

## Existing termination surface

No BARE schema, protocol generator, sidecar service, or actor contract change is
needed:

| Operation | Direct TypeScript backend | Actor backend |
|---|---|---|
| Kill spawned process | `AgentOs.killProcess`, `packages/core/src/agent-os.ts:2123-2131` | generated `killProcess` action and `crates/agentos-actor-plugin/src/actions/process.rs:93-96` |
| Close/kill PTY shell | `AgentOs.closeShell`, `packages/core/src/agent-os.ts:2017-2033` | generated `closeShell` action and `crates/agentos-actor-plugin/src/actions/shell.rs:350-353` |

`ShellVmHandle` at `packages/shell/src/actor-vm.ts:43-72` currently includes
write, stdin-close, and wait methods but omits `killProcess` and `closeShell`.
The actor adapter returned at lines 372-458 likewise forwards write/wait but not
those two already-supported actions.

## Exact production edits

### Add `packages/shell/src/serialized-input.ts`

Add a dependency-free helper with an API approximately like this:

```ts
export interface SerializedInputOperations<T> {
	write(data: T): Promise<void>;
	end(): Promise<void>;
	abort(): Promise<void>;
}

export interface SerializedInputForwarder<T> {
	write(data: T): void;
	end(): void;
	fail(error: unknown): void;
	readonly failed: Promise<void>;
	readonly failure: Promise<never>;
}

export function createSerializedInputForwarder<T>(
	operations: SerializedInputOperations<T>,
): SerializedInputForwarder<T>;
```

Implementation requirements:

- Keep one rejected queue chain. Attach observers with `void queue.catch(...)`,
  but do not replace the queue with a recovered `.catch`, because already
  queued operations must remain skipped after a failure.
- Track `accepting` separately. `end()` changes it to false before queuing EOF,
  making EOF idempotent and preventing data after EOF.
- Route a host `process.stdin` error to `fail(error)`, not to `end()`. A host
  stream failure is not EOF and its original error must remain visible.
- On the first queue rejection, set terminal state synchronously before awaiting
  `abort()`. Resolve `failed` immediately, then begin abort. Multiple observers
  may see the same rejection; only the first may signal or abort.
- After abort succeeds, reject `failure` with the original exact error object.
  If abort rejects, use `new AggregateError([inputError, abortError], ... )`.
  Attach an internal rejection observer so a naturally exited command cannot
  create an unhandled rejection from a later host-input failure; the public
  promise must still retain and reject with the exact final error.
- Never log inside the helper. `failure` is the one observable result, avoiding
  duplicate stderr output and making unit tests deterministic.
- Do not add retries, timeouts, buffering policy, input transformation, or
  process-state inference. Protocol request limits/timeouts and process
  lifecycle remain sidecar-owned.

The important implementation shape is:

```ts
let queue = Promise.resolve();
let accepting = true;
let terminal = false;

const enqueue = (operation: () => Promise<void>) => {
	queue = queue.then(operation);
	void queue.catch(beginFailure);
};
```

`beginFailure(error)` is also the implementation of the public `fail(error)`;
it performs the one synchronous terminal-state transition, resolves `failed`,
and starts `abortAndRejectFailure`.

The observer must not consume the rejection assigned to `queue`. For example,
if `end()` was queued synchronously after a write but before that write rejects,
the EOF operation must be skipped and `abort()` must provide the terminal
cleanup.

### `packages/shell/src/actor-vm.ts`

Extend `ShellVmHandle` after its current process wait method and shell wait
method:

```ts
killProcess(pid: number): Promise<void>;
closeShell(shellId: string): Promise<void>;
```

In the actor adapter returned by `createActorShellVm`, add direct forwarding:

```ts
async killProcess(pid) {
	await handle.killProcess(pid);
},
async closeShell(shellId) {
	await handle.closeShell(shellId);
},
```

`AgentOs` already structurally satisfies the enlarged interface. Do not add
fallback signal logic in `actor-vm.ts`; the actor action and sidecar already own
the exact kill request and response.

### `packages/shell/src/main.ts`: spawned commands

At `runSpawnedCommand` (current lines 506-560), delete `stdinQueue`,
`queueStdin`, and the swallowed `closeProcessStdin` catch. Construct the shared
forwarder after spawn:

```ts
const stdin = createSerializedInputForwarder({
	write: (data: Uint8Array | string) =>
		vm.writeProcessStdin(child.pid, data),
	end: () => vm.closeProcessStdin(child.pid),
	abort: () => vm.killProcess(child.pid),
});
```

Wire host data/end/error events to `stdin.write(data)`, `stdin.end()`, and
`stdin.fail(error)` respectively. For
`!options.interactive`, call `stdin.end()` immediately. In both branches, await
the first authoritative exit or the immediate failure signal, then await final
abort completion if input failed:

```ts
const outcome = await Promise.race([
	vm.waitProcess(child.pid).then((exitCode) => ({ tag: "exit", exitCode }) as const),
	stdin.failed.then(() => ({ tag: "inputFailed" }) as const),
]);
if (outcome.tag === "inputFailed") return await stdin.failure;
return outcome.exitCode;
```

This two-phase shape is load-bearing. A single race against a rejection that is
delayed until after `killProcess` completes is unsafe: the kill's process-exit
event could resolve `waitProcess` first and incorrectly return exit code 137 as
a successful CLI result. The immediate `failed` signal wins before abort begins;
`failure` then makes cleanup complete and preserves its error. Keep listener
removal and stdin pause in `finally`.

### `packages/shell/src/main.ts`: terminal commands

At `runTerminalAttempt` (current lines 588-686), replace `stdinQueue` and
`queueShellInput` with:

```ts
const stdin = createSerializedInputForwarder({
	write: (data: Uint8Array | string) => vm.writeShell(shellId, data),
	end: () => vm.writeShell(shellId, "\u0004"),
	abort: () => vm.closeShell(shellId),
});
```

Route data, EOF, and error events to `write`, `end`, and `fail` respectively. When
`options.interactive === false`, call `stdin.end()` once instead of attaching
listeners. Use the same two-phase `waitShell` versus `stdin.failed` outcome, and
await `stdin.failure` when failure starts. Only perform the existing
trailing-output flush after a successful shell exit. Keep output unsubscribe,
raw-mode restoration, resize-listener removal, and stdin cleanup in `finally`.

Do not convert terminal EOF into a client-side signal or shell parser. The
existing EOT write is the normal terminal input operation; `closeShell` is only
the fail-closed abort after that operation cannot be delivered.

## Exact test work

### Before test

In the parent revision, temporarily add a focused test in
`packages/shell/tests/serialized-input.test.ts` (or an equivalent local
reproduction beside the new helper) using the current queue algorithm:

1. create a deferred first `write`;
2. queue that write and then queue EOF before settling it;
3. reject the write;
4. assert the stderr observer receives the error; and
5. assert the EOF spy was never called and a child-wait deferred remains
   pending over a bounded microtask/short fake-timer window.

This test documents exactly why logging is insufficient. Record it as passing
against the vulnerable parent, then replace it with the after assertions; do not
commit a test that endorses hanging behavior.

### Committed helper tests

Add `packages/shell/tests/serialized-input.test.ts` with deterministic fake
operations and deferred promises. It should cover:

1. **Success ordering:** multiple writes remain serialized, EOF runs after the
   last write, and data/end calls after EOF are ignored.
2. **Write rejection with EOF already queued:** EOF is skipped, `abort` runs
   exactly once, `failed` resolves before abort is released, `failure` rejects
   with the same error object after abort, and no later write runs.
3. **EOF rejection:** `abort` runs exactly once and `failure` rejects; the test
   completes without awaiting a synthetic child exit.
4. **Abort rejection:** `failure` is an `AggregateError` whose `.errors` retain
   the original input error first and cleanup error second.
5. **Repeated observation/races:** multiple queue descendants observing the
   same rejection cannot abort twice or create an unhandled rejection.
6. **Abort-induced exit race:** make `abort` resolve the fake child wait before
   its own promise completes; the runner still selects `inputFailed`, awaits
   abort, and surfaces the input error rather than returning the killed exit
   code.
7. **Host stdin error:** `fail(error)` aborts without attempting normal EOF and
   retains the exact host error.

Use deferred promises and `await Promise.resolve()` to control ordering; do not
use real child processes or long sleep-based timeouts for these unit cases.

### CLI success regressions

Retain the existing real CLI tests in `packages/shell/tests/cli.test.ts`:

- `keeps stdin attached by default` at current lines 67-72 proves normal data
  plus EOF still exits;
- `detaches stdin when --no-interactive is set` at lines 74-82 proves immediate
  spawned-process EOF; and
- terminal command/default-shell tests at lines 84-105 preserve the PTY path.

If a small dependency-injection seam is added around `runSpawnedCommand` and
`runTerminalAttempt`, add one fake-VM wiring test for each abort mapping
(`killProcess(pid)` and `closeShell(shellId)`). Do not export the whole CLI or
import `main.ts` from a unit test: that module executes VM creation at top level.
The pure helper tests plus compile-checked wiring are sufficient if avoiding
that larger refactor.

### Sidecar coverage to retain

Keep, rather than move, the existing sidecar/client lifecycle coverage:

- `packages/core/tests/execute.test.ts:49-55` covers successful write, EOF, and
  wait through the TypeScript sidecar client;
- `crates/native-sidecar/tests/kill_cleanup.rs` covers authoritative process
  kill cleanup; and
- `crates/client/tests/shell_e2e.rs:119-120` covers sidecar shell close.

Those tests prove the supplied abort operations work. They cannot reproduce the
Node CLI's poisoned promise chain and therefore do not replace the shell unit
regression.

## Before and after checklist

### Before behavior

- [ ] A temporary shell test queues a rejected write and EOF using the parent
  algorithm, observes the error text, and proves EOF never invokes its operation.
- [ ] The same test proves the child wait remains pending because logging does
  not settle or terminate it.
- [ ] Source inventory records both independent queues and the swallowed
  `closeProcessStdin` rejection.

Research-time baseline evidence first captured at `0d180ebe3a89` and reproduced
unchanged at `16a41860`:

| Check | Result |
|---|---|
| Standalone Node reproduction of the current `queue = queue.then(operation)` algorithm | **Pass:** a rejected write leaves a synchronously queued EOF callback uncalled. |
| `pnpm --dir packages/shell check-types` | **Environment-blocked before Item 60:** generated declarations for `@rivet-dev/agentos`, `@rivet-dev/agentos/client`, and several registry software packages had not been built. Run the repository build prerequisite before treating this command as implementation evidence. |

The implementation revision should record the temporary before-test name and
output in the tracker. The standalone reproduction above confirms the promise
semantics without modifying or blessing the vulnerable production behavior.

### After behavior

- [ ] Successful writes and EOF preserve strict arrival order in the shared
  forwarder.
- [ ] The first write/EOF failure stops later operations, invokes one abort, and
  resolves the immediate failure signal before abort, then rejects the
  runner-visible final promise without a hang.
- [ ] Spawned-process failure calls `killProcess(pid)`; terminal failure calls
  `closeShell(shellId)`.
- [ ] An abort failure preserves both errors in `AggregateError`.
- [ ] Host stdin errors are surfaced as terminal failures rather than rewritten
  as EOF.
- [ ] `--no-interactive` immediately ends stdin in both spawned and terminal
  modes.
- [ ] Existing real CLI stdin and terminal smoke tests remain green.
- [ ] Item 60 is marked `done` only after before/after evidence is recorded in
  the tracker.

Focused validation commands:

```sh
pnpm build
pnpm --dir packages/shell exec vitest run tests/serialized-input.test.ts
pnpm --dir packages/shell check-types
pnpm --dir packages/shell test
cargo test -p agentos-client --test shell_e2e
cargo test -p agentos-native-sidecar --test kill_cleanup
cargo check --workspace
git diff --check
```

`pnpm build` supplies the workspace declarations and generated registry package
artifacts required by the shell's standalone type-check. The Rust E2E and
workspace check are explicit expensive validation; the focused shell helper
test should be the inner implementation loop.

## Client-to-sidecar test migration

No test should move from the shell package to the sidecar for Item 60. The bug
is specifically the host Node event/promise queue between `process.stdin` and
sidecar requests. The sidecar cannot observe whether the host dropped an EOF
callback from a rejected JavaScript promise chain.

The correct boundary is:

```text
Node CLI: observe host stdin, serialize host callbacks, fail closed
sidecar: deliver bytes/EOF, terminate process/shell, report authoritative exit
```

Keep sidecar kill, stdin-close, terminal-close, and exit tests where they are.
Do not add another queue or EOF policy to the sidecar merely to compensate for
the CLI dropping a request before sending it.

## Dependencies and overlap

- **Item 59 is adjacent but independent.** It addresses finite `exec` input in
  the TypeScript and Rust SDKs, ideally with one sidecar operation. Item 60 is a
  genuinely streaming host CLI and cannot package an unbounded interactive
  terminal session into that finite request. Reuse naming/error patterns if
  Item 59 lands first, but do not wait for or duplicate its protocol work.
- **Item 65 provides the cleanup-error principle.** Item 60 can use standard
  `AggregateError` immediately; it should not flatten or silently log an abort
  failure while waiting for the broader cleanup audit.
- **Item 66 also edits `packages/shell/src/main.ts`.** Its package-selection
  probes are unrelated. Stack the revisions and preserve its removal of host
  filesystem inspection while changing only command-input sections here.
- The actor `killProcess` and `closeShell` contracts are already generated and
  implemented. If an implementation attempts to edit actor action generation,
  the BARE protocol, or native sidecar routing, it has exceeded Item 60's scope.
- No Rust client parity edit is required. The numbered defect is in the
  TypeScript Node shell CLI; Rust has no equivalent host CLI stdin event loop.

## Risks and review points

- **A `.catch` used only for logging is not recovery.** The command runner must
  observe the immediate failure signal, await the final rejected failure
  promise, and start termination before it can remain stuck on
  `waitProcess`/`waitShell`.
- **Do not let abort-induced exit win the race.** Signal failure before invoking
  kill/close, then await abort and throw the retained error. Otherwise a fast
  process-exit event can turn an input failure into an apparently normal 137.
- **Do not recover the queue and run EOF after an uncertain write.** Once a
  sidecar input request fails, ordering is no longer trustworthy. Make the
  stream terminal and abort the process/shell.
- **Do not swallow `closeProcessStdin`.** An already-exited process should be
  recognized by the authoritative wait/sidecar behavior, not by ignoring every
  close failure.
- **Abort once.** Every descendant of a rejected promise can run its rejection
  observer. Set terminal state synchronously before awaiting the abort RPC.
- **Preserve cleanup failure.** If kill/close also fails, retaining only the
  first error hides whether the process was actually terminated.
- **Do not create terminal emulation.** Normal PTY EOF remains an EOT input; the
  sidecar terminal implementation owns line discipline and signals.
- **Keep listeners bounded by the command lifetime.** Existing `finally`
  listener removal, stdin pause, raw-mode reset, resize cleanup, and output
  unsubscribe must remain intact.

## Bounded dedicated JJ revision

Create one new stacked JJ revision for Item 60 and keep it to:

```text
packages/shell/src/serialized-input.ts
packages/shell/src/main.ts
packages/shell/src/actor-vm.ts
packages/shell/tests/serialized-input.test.ts
docs/thin-client-migration.md  # evidence/status only, last
```

`packages/shell/tests/cli.test.ts` should change only if a narrowly injected
abort-wiring test can be added without importing the top-level executable.
No protocol schema/generated source, Rust product client, native/browser
sidecar, package manifest, dependency, lockfile, website, or secure-exec mirror
edit is expected.
