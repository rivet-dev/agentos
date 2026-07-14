# Item 74 research: reject process starts after the TypeScript event pump fails

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 74 tracker status.

## Recommendation

Make a genuine `NativeSidecarKernelProxy` event-pump failure terminal for later
process starts:

1. reject before `Execute` when `pumpError` is already known;
2. check again after mount reconfiguration, so a failure while waiting cannot
   reach `Execute`;
3. after `Execute` resolves, check once more before registering the route; and
4. if that final check finds a failure, send `SIGKILL` for the returned sidecar
   process ID and reject the spawn with the original pump error.

The two sides of the final `await this.client.execute(...)` are the required
linearization points. No mutex, pump restart, retry, client timeout, sidecar
default, or protocol change is needed.

Priority: **P1**. Confidence: **high**.

## Original issue

The numbered summary, status row, and checklist are currently at
`docs/thin-client-migration.md:120,202,289`:

> After the TypeScript sidecar event pump fails, `startTrackedProcess` can still
> start a new process even though no consumer remains to deliver its output or
> exit, so the new process can hang indefinitely.

`NativeSidecarKernelProxy` starts one VM-scoped event pump in its constructor at
`packages/core/src/sidecar/rpc-client.ts:238-265`. A genuine
`waitForEvent`/transport failure is retained in `pumpError` and permanently ends
that pump at current lines 860-879. Item 69 will make that catch fail existing
routes with the exact error and report it once, but it deliberately does not
restart the pump.

`startTrackedProcess` at current lines 777-804 never reads `pumpError`. It waits
for mount reconfiguration, sends `Execute`, then adds the returned process to
`trackedProcessesById` and `trackedProcesses`. Consequently a successful
`spawn` can return a handle whose `wait()` will never receive a terminal event.

The stored error is currently write-only: the only repository occurrences of
`pumpError` are its field declaration and assignment/use in the failed pump.

## Cross-layer inventory

| Layer | Exact location | Item 74 disposition |
| --- | --- | --- |
| TypeScript VM proxy state | `packages/core/src/sidecar/rpc-client.ts:214-265` | `pumpError`, the two route maps, and the one VM-scoped pump are the only state this item needs. Add no new long-lived state. |
| TypeScript process creation | `packages/core/src/sidecar/rpc-client.ts:460-520,777-804` | Guard `startTrackedProcess` before/after its existing awaits, clean up a returned process before route publication, and keep `spawn`'s public payload unchanged. |
| TypeScript pump failure | `packages/core/src/sidecar/rpc-client.ts:806-919` | Keep one terminal stored error. Item 69 owns callback isolation/diagnostics; Item 74 consumes the genuine failure state for new starts. |
| TypeScript wire client | `packages/runtime-core/src/sidecar-process.ts:1453-1517,1598-1622` | No edit. `execute` already returns `{ processId, pid }`, and `killProcess` already accepts an exact process ID and signal. |
| Wire schema | `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:373-407,674-678,694-696,969-975` | No edit. Execute/start, kill, acknowledgement, and terminal-event correlation already exist. |
| Generated TypeScript protocol | `packages/runtime-core/src/generated-protocol.ts:2118-2166,2219-2234,3409-3430,4869-4893` | No edit or regeneration. Existing codecs carry every field needed by the fix. |
| Native/browser sidecars | `crates/native-sidecar/src/execution.rs:4282-4294,4645-4676`; `crates/native-sidecar-browser/src/wire_dispatch.rs:2163-2219` | No edit. Both accept exact-ID kill; browser treats an inactive ID as already complete, and native does the same while the terminal snapshot is retained. |
| Rust client | `crates/client/src/process.rs:697-740,767-777,817-887` | No edit. Rust atomically binds an Execute response to its event subscription and already uses `SIGKILL` after a route failure. |
| Focused TypeScript tests | `packages/core/tests/leak-rpc-client.test.ts:22-252`; `packages/core/tests/process-event-ordering.test.ts` | Extend the stub lifecycle suite for the two race orderings and cleanup failure. Keep Item 59/69 ordering coverage green. |

This inventory is why the fix belongs entirely in the TypeScript host adapter.
The sidecar remains the process authority; the client only refuses to publish a
host handle it knows it can no longer drive, and uses the existing Linux signal
surface to fail-close a process already accepted by the sidecar.

## Exact failing interleavings

### Failure already known before start

```text
event pump                    spawn/startTrackedProcess
----------                    -------------------------
waitForEvent rejects
pumpError = transportError
pump returns permanently
                              Execute -> { processId, pid }
                              register both process maps
                              return ManagedProcess
                              wait() hangs: no pump exists
```

The fixed path rejects before `Execute`, so it creates neither a sidecar process
nor a local route.

### Pump fails while Execute is in flight

```text
event pump                    spawn/startTrackedProcess
----------                    -------------------------
                              Execute request is pending
waitForEvent rejects
pumpError = transportError
no route exists to fail
pump returns permanently
                              Execute resolves
                              register orphan route
```

The fixed path observes `pumpError` immediately after `Execute`, sends
`KillProcess(SIGKILL)` for the returned `processId`, rejects the start, and does
not insert either route-map entry.

### Execute continuation wins the race

If `Execute` resolves first, its JavaScript continuation performs the post-check
and both map insertions synchronously in one turn. The pump catch can then run
only after registration, sees the entry, and fails it normally. There is no
interleaving inside those synchronous statements, so a mutex would add state
without closing another gap.

The post-`Execute` check is also sufficient for any number of concurrent starts:
every response that arrives after the pump has recorded failure cleans up its
own returned process before it can become a route.

## Exact production edit

Change only `NativeSidecarKernelProxy` in
`packages/core/src/sidecar/rpc-client.ts`.

Add a small exact-error guard:

```ts
private throwIfEventPumpFailed(): void {
	if (this.pumpError) {
		throw this.pumpError;
	}
}
```

Add a cleanup helper for a process that was started after route supervision was
lost:

```ts
private async abortStartedProcessAfterPumpFailure(
	processId: string,
	pumpError: Error,
): Promise<never> {
	try {
		await this.client.killProcess(
			this.session,
			this.vm,
			processId,
			"SIGKILL",
		);
	} catch (cleanupError) {
		throw new AggregateError(
			[pumpError, toError(cleanupError)],
			`failed to abort sidecar process ${processId} after event pump failure`,
		);
	}
	throw pumpError;
}
```

Then wrap the existing request construction without changing its payload:

```ts
private async startTrackedProcess(entry: TrackedProcessEntry): Promise<void> {
	this.throwIfEventPumpFailed();
	await this.waitForMountReconfigure();
	this.throwIfEventPumpFailed();

	const started = await this.client.execute(
		this.session,
		this.vm,
		{
			// Keep the existing request fields exactly as they are.
		},
	);

	const pumpError = this.pumpError;
	if (pumpError) {
		await this.abortStartedProcessAfterPumpFailure(
			started.processId,
			pumpError,
		);
	}

	if (started.pid === null) {
		throw new Error("sidecar did not return a kernel pid for the process");
	}
	entry.processId = started.processId;
	entry.pid = started.pid;
	this.trackedProcessesById.set(entry.processId, entry);
	this.trackedProcesses.set(entry.pid, entry);
}
```

The post-check must precede the null-PID validation. `Execute` may have started a
real process and supplied its `processId` even when the response is otherwise
malformed; the route failure is already known, so that process must be aborted
before reporting response validation.

Use `SIGKILL`, not a graceful default. Once terminal-event supervision is gone,
the client cannot safely wait for graceful termination or escalation. The Rust
client already names the same invariant with
`ROUTE_FAILURE_KILL_SIGNAL = "SIGKILL"` in
`crates/client/src/process.rs:29,767-777`.

On successful cleanup, throw the **same `pumpError` object**, preserving its
typed fields and identity. If cleanup itself fails, report both failures in an
`AggregateError`, with the original pump error first and the exact normalized
cleanup error second. Do not swallow the cleanup rejection or pretend the
process was terminated.

Do not call `failProcess(entry)` on this branch. `spawn` has not returned a
`ManagedProcess`, so nobody can observe `entry.waitPromise`; rejecting it would
create an unhandled rejection. Leaving the never-published entry unreachable is
safe, and its listener sets and closures are garbage-collectable because it was
never inserted into either tracking map.

## Why this stays in the TypeScript client

This is not VM behavior or process policy to move into the sidecar. It is a
TypeScript host-route lifecycle defect: the sidecar accepted `Execute`, but only
the client knows its local consumer promise has ended and can no longer route
the sidecar's already-emitted process events.

The Rust client does not share this architecture. `AgentOs::send_execute` at
`crates/client/src/process.rs:697-734` calls
`request_wire_with_process_events`, which installs and returns the exact process
event subscription atomically with the `Execute` response. A missing binding is
already an error. Its per-process pump then handles typed lag/close failure.
Therefore Item 74 needs no Rust-client parity edit and no sidecar or wire-protocol
surface.

Do not try to restart the TypeScript pump. Without a replay cursor and
acknowledgement contract, restart can silently skip or duplicate output and exit
events. Do not clear `pumpError`, retry `Execute`, synthesize an exit code, add a
timer, or teach the sidecar about a client's local promise state.

## Before/after tests

Extend `packages/core/tests/leak-rpc-client.test.ts`. Its existing stub already
exposes `failPump`, exposes tracking sizes through the proxy, and covers failure
of an existing process. Add deterministic deferred-`Execute`/`configureVm`
control and call spies to that stub as needed; do not add a production test
hook.

| Regression | Before-fix proof | After-fix proof |
|---|---|---|
| Start after known pump failure | Fail the pump, wait until the catch has recorded/reported it, then call `spawn`. Current code calls `execute` and resolves a process handle despite there being no consumer. | `spawn` rejects with the exact pump error; `execute` and `killProcess` are not called; both tracking-map sizes remain zero. |
| Pump fails during mount reconfiguration | Hold the stub's `configureVm`, begin `spawn` so it blocks in `waitForMountReconfigure`, fail the pump, then release configuration. Current code proceeds to `Execute`. | The post-reconfiguration guard rejects with the exact pump error; `execute` and `killProcess` are not called and both maps remain empty. |
| Pump fails while Execute is pending | Hold `execute` on a deferred promise, begin `spawn`, fail the pump, wait until failure is observed, then resolve `{ processId: "process-race", pid: 4242 }`. Current code resolves `spawn`, performs no kill, and leaves one route in each map. | Attach the rejection assertion before resolving the deferred promise. `spawn` rejects with the exact pump error, `killProcess(session, vm, "process-race", "SIGKILL")` is called once, and both maps remain empty. |
| Execute continuation wins | Resolve deferred `Execute` before rejecting the pump so registration's microtask runs first. | `spawn` may return its handle, but its `wait()` rejects with the exact pump error and both maps are released; the post-check does not double-kill. This demonstrates the other race ordering is already closed by JavaScript run-to-completion. |
| Race cleanup fails | In the pump-first race, make `killProcess` throw a distinct error. | `spawn` rejects an `AggregateError` whose `.errors` are exactly `[pumpError, cleanupError]`; neither map gains an entry and no unobserved `waitPromise` rejection occurs. |

Suggested test names are
`rejects a start after the event pump has failed`,
`rechecks pump failure after mount reconfiguration`,
`kills an untracked process when the pump fails during Execute`,
`fails a route when Execute registration wins the pump race`, and
`preserves pump and cleanup failures when race cleanup fails`.

For pump-first tests, use an explicit barrier that proves the pump catch ran
before releasing `configureVm` or `Execute`. The smallest current-tree barrier
is `vi.waitFor` on the exact private field in a test-only cast:

```ts
await vi.waitFor(() =>
	expect(
		(proxy as unknown as { pumpError: Error | null }).pumpError,
	).toBe(pumpError),
);
```

If Item 69 lands first, its one host-visible pump diagnostic may be used as the
barrier instead. A fixed number of arbitrary sleeps would make the race tests
flaky. Make `execute`, `killProcess`, and `configureVm` `vi.fn` stubs, and use a
small reusable deferred helper rather than production hooks. Keep the existing
test that a process already registered when the pump fails has its `wait()`
rejected and routes released.

For tracker evidence, write the desired assertions first and run the focused
file against the vulnerable parent. The known-failure, post-mount, pump-first
Execute, and cleanup-failure cases must be red for the reasons in the table;
retain those same tests and make them green with the production edit. The
Execute-wins test is positive race coverage and should remain green on both
sides.

## Research baseline

On research revision `42c9f0e97cc1`, the existing focused suite is green:

```text
pnpm --dir packages/core exec vitest run tests/leak-rpc-client.test.ts
  1 file passed; 5 tests passed; 0 failed
```

Those five tests cover existing-route failure and disposal cleanup, but none
starts a process after or concurrently with pump failure. The durable red/green
cases above supply the missing Item 74 evidence.

## Dependencies and adjacent scope

- **Stack after Item 69.** Item 69 ensures a throwing output callback cannot
  falsely poison the pump and establishes the genuine pump-failure diagnostic
  used as a deterministic test barrier. Preserve its removal of fabricated
  stderr.
- **Item 59 may touch the same Execute payload.** Keep every request field and
  omission behavior from the current stacked parent; Item 74 wraps the request
  rather than rebuilding it.
- **Item 65 establishes cleanup aggregation conventions.** Reuse its
  `AggregateError`/original-error ordering if it lands first. Item 74 must not
  flatten the pump and kill failures into one message.
- **Item 70 touches the same proxy class.** Its removal of the duplicate public
  process snapshot cache does not remove either active route map used here;
  preserve the Item 70 shape rather than reintroducing that cache.
- **Item 71 may touch terminal retention/kill parity.** Cleanup must use only the
  returned process ID. Treat a successful already-terminal kill acknowledgement
  as cleanup success, and propagate any real rejection without parsing native
  or browser message text.
- **Item 77 touches proxy disposal and transport termination.** Rebase around
  its lifecycle gate if it lands first, but do not fold VM disposal/start
  serialization into this process-route race revision.
- **Item 22 is related but complete.** It owns Rust transport route loss and
  atomic request/route binding; do not duplicate its protocol machinery here.
- A simultaneous `dispose()`/start and termination of processes that were
  already tracked when the pump failed have similar lifecycle questions, but
  are not Item 74. Do not silently broaden this small race fix into VM teardown
  policy.

## Dedicated JJ revision and bounded paths

Implement Item 74 in one dedicated stacked child revision:

```text
packages/core/src/sidecar/rpc-client.ts
packages/core/tests/leak-rpc-client.test.ts
docs/thin-client-migration.md       # checklist/status only after validation
```

Suggested description:

```text
fix(client): reject starts after event pump failure
```

Do not edit Rust, sidecar, protocol, generated protocol, public types, or docs
outside the tracker checklist for this item.

## Validation

Run the focused regression first, then the neighboring Item 69 ordering suite
and package checks:

```sh
pnpm --dir packages/core exec vitest run tests/leak-rpc-client.test.ts
pnpm --dir packages/core exec vitest run tests/process-event-ordering.test.ts
pnpm --dir packages/core check-types
pnpm --dir packages/core build
pnpm exec biome check packages/core/src/sidecar/rpc-client.ts \
  packages/core/tests/leak-rpc-client.test.ts
git diff --check
```

The completion checklist should record the exact before-fix assertions, the
five after-fix race/error assertions, the commands above, and the dedicated JJ
revision before changing Item 74 from `pending` to `done`.
