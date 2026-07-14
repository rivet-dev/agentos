# Item 70 research: remove the duplicate TypeScript process snapshot cache

Status: implementation-ready research only. This note does not modify
production code, tests, or the Item 70 tracker status.

Inspected on **2026-07-14** at revision **`d74ae0307141`**. Tracker anchors are
`docs/thin-client-migration.md:116` (issue inventory), current line 197
(pending status), and current line 283 (before/after/complete checklist).

## Recommendation

Delete `NativeSidecarKernelProxy.processes`, stop copying each returned process
snapshot into that map, and remove `Kernel.processes` plus the non-native branch
that reads it. Replace the legacy synchronous `Kernel` inspection surface with
the proxy's existing asynchronous `snapshotProcesses()` method. Public process
queries must continue to return only the response from the sidecar's
`get_process_snapshot` request.

Priority: **P2**. Confidence: **high**.

The desired flow is one-way:

```text
AgentOs.allProcesses/listProcesses/getProcess
                     |
                     v
Kernel.snapshotProcesses()
                     |
                     v
SidecarProcess.getProcessSnapshot() -- wire get_process_snapshot
                     |
                     v
sidecar-owned active + retained-terminal process state
```

Do not replace the map with another cache, event-derived table, fallback, or
client retention policy. Keep the separate host routing maps that correlate
process output/control callbacks; those contain state the sidecar cannot own and
are not used as the public process snapshot.

## Original issue

The tracker entries are at `docs/thin-client-migration.md:116,197,283`:

> `NativeSidecarKernelProxy.processes` retains a second copy of every entry in
> the latest process snapshot but has no production reader because
> `AgentOs.listProcesses/getProcess` use the directly returned authoritative
> snapshot.

Every `snapshotProcesses()` call currently constructs a `ProcessInfo` object for
each sidecar entry, copies the same object references into a persistent
`Map<number, ProcessInfo>`, and also returns them in a new array. The caller uses
the returned array. The map merely retains the latest snapshot after the caller
is finished and adds one map node/key per process.

The only apparent production read is the fallback branch in
`AgentOs.allProcesses()`:

```ts
if (this.#kernel instanceof NativeSidecarKernelProxy) {
	return await this.#kernel.snapshotProcesses();
}
return [...this.#kernel.processes.values()];
```

All production `AgentOs` VMs install `NativeSidecarKernelProxy`, so that fallback
does not read the proxy map. It is a stale abstraction from the former alternate
kernel/runtime path and advertises synchronous, potentially stale process state
through the repo-internal `Kernel` type and its test-only runtime escape hatch.
`Kernel` is not exported from the package root.

## Exact current code and data flow

### Persistent duplicate in `NativeSidecarKernelProxy`

`packages/core/src/sidecar/rpc-client.ts:214-241` declares three different
process-related state groups:

- `readonly processes = new Map<number, ProcessInfo>()` at line 218;
- `trackedProcesses` and `trackedProcessesById` at lines 227-231; and
- `processSnapshotRefresh` at lines 232-234.

Only the first is Item 70's duplicate cache.

`snapshotProcesses` at current lines 747-749 awaits
`refreshProcessSnapshot()` and passes the result to `buildProcessSnapshot`.
`processSnapshotById` at lines 751-757 searches the wire snapshot directly by
the sidecar process correlation ID; it never reads `this.processes`.

`buildProcessSnapshot` at current lines 1006-1034:

1. creates a local `Map<number, ProcessInfo>`;
2. translates each `SidecarProcessSnapshotEntry` to `ProcessInfo`;
3. clears the persistent `this.processes` map;
4. inserts every translated object into the persistent map; and
5. returns a separately allocated, PID-sorted array from the local map.

Repository-wide production search finds no `this.processes` read. Its only
mutations are the clear/insert block in `buildProcessSnapshot`.

### `processSnapshotRefresh` is not the cache being removed

`refreshProcessSnapshot` at `rpc-client.ts:759-775` stores only the currently
in-flight promise. Concurrent callers share one wire request; the `finally`
block sets the field back to `null` on both success and failure. A later call
always sends another `get_process_snapshot` request.

Retain this single-flight guard. It prevents two simultaneous host reads from
issuing duplicate requests but does not preserve snapshot entries or provide a
stale fallback. Deleting it would increase wire traffic without reducing
retained process state.

### The public methods already use the returned snapshot

`packages/core/src/agent-os.ts` reads process data as follows:

- `listProcesses` at current lines 2035-2055 calls `allProcesses`, indexes that
  returned array by PID, and joins it with `_processes` keys;
- `allProcesses` at lines 2057-2063 calls
  `NativeSidecarKernelProxy.snapshotProcesses()` in every production VM;
- `getProcess` at lines 2089-2109 verifies that the PID is a caller-spawned
  route in `_processes`, then searches a fresh `allProcesses` response.

Item 41 removes `processTree` before this stacked item lands. If Item 70 is
temporarily applied to an earlier parent, that method also delegates through
`allProcesses` and does not read the duplicate cache.

None of those methods read `NativeSidecarKernelProxy.processes`. `_processes` is
different: it contains running/completed/failed host routes for processes
created through `AgentOs.spawn`, including callbacks and terminal results. It
is bounded by the sidecar-advertised route retention and is needed to distinguish
caller-spawned processes from all guest/kernel processes. Do not delete it in
Item 70.

### The sidecar is authoritative

`SidecarProcess.getProcessSnapshot` in
`packages/runtime-core/src/sidecar-process.ts:1653-1674` sends a VM-owned
`get_process_snapshot` request, validates a `process_snapshot` response, and
returns its entries. It does not cache them.

The native sidecar handler in
`crates/native-sidecar/src/execution.rs:4321-4348`:

- validates VM ownership;
- enforces `process.inspect` permission;
- prunes authoritative retained terminal snapshots according to the configured
  bound;
- snapshots the live kernel process table plus retained exits; and
- returns the response.

The snapshot builder at `execution.rs:12752-12780` combines current active
process trees with the sidecar's bounded `exited_process_snapshots`. That is the
only process-retention policy Item 70 should preserve.

The browser sidecar implements the same wire request from its own live kernel
state. Do not change native/browser process semantics in this TypeScript cache
deletion.

## Exact production edits

### `packages/core/src/sidecar/rpc-client.ts`

1. Delete the field:

   ```ts
   readonly processes = new Map<number, ProcessInfo>();
   ```

2. Simplify `buildProcessSnapshot` to a direct translation plus the existing PID
   sort:

   ```ts
   private buildProcessSnapshot(
     snapshot: SidecarProcessSnapshotEntry[],
   ): ProcessInfo[] {
     return snapshot
       .map((entry) => ({
         pid: entry.pid,
         ppid: entry.ppid,
         pgid: entry.pgid,
         sid: entry.sid,
         driver: entry.driver,
         command: entry.command,
         args: entry.args,
         cwd: entry.cwd,
         status: entry.status,
         exitCode: entry.exitCode,
         startTime: entry.startTime,
         exitTime: entry.exitTime,
       }))
       .sort((left, right) => left.pid - right.pid);
   }
   ```

   This also removes the transient `processMap`, which was silently
   deduplicating duplicate PIDs. PID uniqueness is a sidecar/kernel invariant;
   the client should not hide a malformed snapshot with an independent
   last-entry-wins policy.

3. Keep `snapshotProcesses`, `processSnapshotById`, and
   `refreshProcessSnapshot` unchanged. In particular, retain the `finally` that
   resets `processSnapshotRefresh` after each in-flight request.

Do not touch `trackedProcesses`, `trackedProcessesById`, their bounded terminal
cleanup, or output listener sets. They route process handles/events and do not
duplicate the public sidecar snapshot.

Update the comment in `finishProcess` at current lines 895-898 so it no longer
says the exited record lives in the deleted client `processes` map. State that
the exited record remains in the **sidecar-owned bounded process snapshot** for
late listing. This is a comment-only correction; do not retain a client table to
make the old wording true.

### `packages/core/src/runtime.ts`

Replace the legacy synchronous map in `Kernel`:

```ts
readonly processes: ReadonlyMap<number, ProcessInfo>;
```

with the authoritative asynchronous query already implemented by the native
proxy:

```ts
snapshotProcesses(): Promise<ProcessInfo[]>;
```

This makes the internal type express the real transport boundary.
`getAgentOsKernel()` is exposed only through the package's `./test/runtime`
entrypoint, so leaving `processes` in the interface after deleting the runtime
field would still promise repo integration tests a property that is `undefined`
at runtime. No root public export change is required.

Do not add both members or mark `processes` optional. Either choice preserves a
tempting stale fallback and weakens compile-time enforcement of the thin-client
contract.

### `packages/core/src/agent-os.ts`

Replace the entire conditional in `allProcesses` with:

```ts
async allProcesses(): Promise<KernelProcessInfo[]> {
	return await this.#kernel.snapshotProcesses();
}
```

This keeps errors from `get_process_snapshot` visible to the caller. Do not
catch them and return an empty array or host-route state. `listProcesses` and
`getProcess` should continue to delegate through `allProcesses` without
additional changes. Item 41 removes `processTree` earlier in the stack; do not
reintroduce it here.

No production sidecar, protocol, runtime-core, Rust-client, actor, or browser
edit is needed.

## Before and after tests

Add `packages/core/tests/process-snapshot-forwarding.test.ts` with a stub
`SidecarProcess`, using the same lightweight proxy construction and abortable
`waitForEvent` pattern as `leak-rpc-client.test.ts` and
`process-event-ordering.test.ts`.

Use snapshot entries with distinct PIDs and every field populated so conversion
and sorting remain covered. Always dispose the proxy in `finally` so its event
pump is aborted.

### Before-behavior source/heap evidence

On the vulnerable parent, have `getProcessSnapshot` return a large deterministic
array (for example 4,096 entries), call `snapshotProcesses`, then assert:

```ts
expect(proxy.processes.size).toBe(4_096);
expect([...proxy.processes.values()]).toEqual(returnedSnapshot);
```

Also assert corresponding objects by identity. This proves the proxy retains
one map node/key/reference for every returned object after the request has
finished, despite the caller using only the returned array. Record the focused
passing command and vulnerable revision in the tracker. Do not keep an
artificial memory-heavy test in the final suite.

### After: no persistent cache and exact forwarding

The lasting focused test should:

1. return two unsorted complete entries from `getProcessSnapshot`;
2. call `proxy.snapshotProcesses()`;
3. assert `getProcessSnapshot` received the exact session and VM handles once;
4. assert the returned `ProcessInfo[]` contains every field and is sorted by
   PID, preserving current public behavior; and
5. assert `Object.hasOwn(proxy, "processes")` is `false`.

Add a type assertion that `Kernel` has `snapshotProcesses` and no `processes`
member, importing the internal type directly from `../src/runtime.js`. For
example, assert that `"snapshotProcesses" extends keyof Kernel` is `true` and
`"processes" extends keyof Kernel` is `false`. `packages/core` type-checking
must fail if the old map is reintroduced as the interface fallback.

### After: snapshots remain live, not cached

Have the stub return snapshot A on the first call and a different snapshot B on
the second sequential call. Assert:

- `getProcessSnapshot` was called twice;
- the second result contains only B; and
- no property on the proxy retains A as a process table.

Add one concurrent call case for `snapshotProcesses()` and
`processSnapshotById()` with a deferred stub response. Assert they share one
`getProcessSnapshot` call while it is in flight, then a subsequent call issues a
second request. This protects the useful `processSnapshotRefresh` single-flight
behavior from being mistaken for the removed persistent cache.

### Retain public integration coverage

Run, without rewriting:

- `packages/core/tests/process-management.test.ts` for `listProcesses` and
  `getProcess` against real sidecar snapshots;
- `packages/core/tests/all-processes.test.ts` for all-runtime process snapshots
  and parent/child relationships;
- `packages/core/tests/session-cleanup.test.ts` for direct sidecar inspection;
  and
- the sidecar process-snapshot tests in
  `packages/core/tests/native-sidecar-process.test.ts`.

Do not move snapshot construction or retention tests out of the sidecar. The new
unit test covers only absence of duplicate TypeScript state and preservation of
the wire request.

## Validation commands

Run the cheap focused gates first:

```bash
pnpm --dir packages/core exec vitest run \
  tests/process-snapshot-forwarding.test.ts \
  tests/leak-rpc-client.test.ts \
  tests/process-event-ordering.test.ts \
  --fileParallelism=false
pnpm --dir packages/core check-types
```

Then run the real sidecar process coverage:

```bash
pnpm --dir packages/core exec vitest run \
  tests/process-management.test.ts \
  tests/all-processes.test.ts \
  tests/native-sidecar-process.test.ts \
  --fileParallelism=false
pnpm check-types
git diff --check
```

The second group starts native VMs and is intentionally more expensive.

## Dependencies, overlaps, and risks

- **Item 18.33 is the precedent.** It removed socket/signal/zombie diagnostic
  caches in favor of awaited sidecar queries. Item 70 applies the same rule to
  the remaining process snapshot map.
- **Item 32 established late shell reads.** `waitShell` and `closeShell` call
  `processSnapshotById`; keep that direct wire-backed lookup and do not replace
  it with a client map.
- **Item 41 must precede Item 70 in the numbered stack.** It deletes the unused
  client-built `processTree` APIs. Preserve that deletion and keep this item
  limited to the remaining flat `allProcesses`, `listProcesses`, and
  `getProcess` paths.
- **Item 69 overlaps `rpc-client.ts`.** Preserve its output-listener isolation
  changes. Item 70 touches only the field, snapshot translator, and tests.
- **Item 71 owns terminal expiry semantics.** Do not infer process expiration
  from snapshot absence, change native/browser retention, or add a client
  tombstone here. Item 70 only removes a non-authoritative copy.
- **Item 72 is Rust-only route compaction.** Rust's `processes` collection holds
  SDK-spawned control/output routes, not a copy of its latest public snapshot.
  Do not delete it for parity with this unrelated TypeScript field.
- **Preserve single-flight behavior.** `processSnapshotRefresh` must clear after
  resolve/reject and must never become a lasting cache.
- **Preserve sidecar errors.** `process.inspect` denial, ownership rejection,
  transport failure, and malformed response must continue to reject the public
  query; no fallback may hide them.
- **Preserve PID ordering.** `buildProcessSnapshot` currently sorts ascending.
  The direct mapper should keep that observable behavior.
- **Do not delete host route maps.** `_processes`, `trackedProcesses`, and
  `trackedProcessesById` are separate bounded callback/control correlation and
  are still required.
- **Removing the legacy `Kernel.processes` type member is intentional.** It is a
  repo-internal/test-only surface rather than a root SDK export, and a
  synchronous map cannot represent an authoritative remote process table.

## Bounded JJ revision

Create one dedicated stacked Item 70 revision containing only:

```text
packages/core/src/sidecar/rpc-client.ts
packages/core/src/runtime.ts
packages/core/src/agent-os.ts
packages/core/tests/process-snapshot-forwarding.test.ts
docs/thin-client-migration.md
```

No Rust, protocol/generated, runtime-core, browser, actor, README, package
manifest, or lockfile edit is expected. Preserve unrelated shared-worktree
changes in the three TypeScript source files and inspect the revision paths
before describing/squashing it.

Recommended revision description:

```text
refactor(core): remove process snapshot cache
```

## Completion checklist

- [ ] Vulnerable-parent evidence proves one retained map entry/reference per
  returned snapshot entry after the wire request completes.
- [ ] `NativeSidecarKernelProxy` has no `processes` field or snapshot cache
  mutation.
- [ ] `Kernel` exposes only asynchronous `snapshotProcesses`, not a synchronous
  process map.
- [ ] `AgentOs.allProcesses` has no legacy kernel fallback.
- [ ] Sequential calls issue fresh sidecar requests; concurrent calls retain
  only the bounded in-flight single-flight behavior.
- [ ] Public process conversion, PID ordering, and exact sidecar error
  propagation remain covered.
- [ ] Existing real process-management and sidecar snapshot suites pass.
- [ ] The dedicated Item 70 `jj` revision contains only the bounded paths above.
- [ ] `docs/thin-client-migration.md` records before evidence, after evidence,
  revision ID, and marks Item 70 `done` only after all checks pass.
