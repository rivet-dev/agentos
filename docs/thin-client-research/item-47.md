# Item 47 research: lease the real TypeScript VM directly

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 47 tracker status.

Inspected: **2026-07-14**, shared working copy `95aedc828e90` (`pzzlonpr`).
Because this is a shared `jj` working copy, use the symbol names and code shapes
below as the stable anchors if subsequent stacked items shift the numeric line
positions.

Focused parent characterization rerun on that revision:

```text
pnpm --dir packages/core exec vitest run tests/sidecar-client.test.ts
Test Files  1 passed (1)
Tests       3 passed (3)
```

## Recommendation

Delete the internal `AgentOsSidecarClient` lifecycle framework and make
`leaseAgentOsSidecarVm` own the real `AgentOsVmAdmin` returned after the sidecar
has initialized the VM. A lease should contain only:

- the public `AgentOsSidecar` handle;
- the real VM admin, which already contains the authoritative wire session and
  sidecar VM identities; and
- an idempotent, retryable `dispose()` that releases that admin and then updates
  the host-owned lease set/refcount.

Retain `AgentOsSidecarState.activeLeases`, `activeVmCount`, the shared process
handle, and the Node event-loop hold count. Those are legitimate host ownership
and pooling state that the sidecar cannot manage for the Node process. Remove the
second session/VM state machine, UUIDs, timestamps, cloned lifecycle records,
and VM-admin map.

Do not add a sidecar RPC for this. The sidecar already creates, identifies, and
disposes the real VM. This item removes client emulation around those RPCs.

Priority: **P2**. Confidence: **medium**, matching the tracker (approximately
0.75 for the complete implementation, although the architectural diagnosis is
high confidence). The deletion and direct replacement are mechanically clear
and the real VM admin already owns the correct disposal operation. The
implementation rating stays medium because shared-process ref/unref, retryable
disposal, creation failure, and sibling-VM isolation are subtle and must retain
their current integration coverage.

## Original issue and current behavior

Line numbers below are from the inspected shared working copy and may move before
Item 47 is implemented.

### The authoritative VM already exists

`AgentOs.create()` builds `createVmAdmin` at
`packages/core/src/agent-os.ts:1406-1559`. That function:

1. obtains the real shared `SidecarProcess` and authenticated wire session;
2. sends `initializeVm` at lines 1477-1485;
3. receives the sidecar-generated `nativeVm.vmId` and authoritative guest
   initialization result, including cwd/env;
4. constructs `NativeSidecarKernelProxy` with the real connection/session/VM
   ownership; and
5. returns `AgentOsVmAdmin`, which contains `sidecarClient`, `sidecarSession`,
   `sidecarVm`, and an admin `dispose()` that ultimately invokes the real
   sidecar VM disposal.

`NativeSidecarKernelProxy.dispose()` at
`packages/core/src/sidecar/rpc-client.ts:278-369` is authoritative and retryable:
it calls `client.disposeVm(session, vm)` first and only tears down local routes
after the sidecar accepts the remote disposal. With `ownsClient: false`, it
correctly leaves the shared sidecar process alive for sibling VMs.

Nothing else is required to identify or own the VM.

### A synthetic lifecycle is then wrapped around it

Despite already having the real admin, `leaseAgentOsSidecarVm` at
`packages/core/src/agent-os.ts:3544-3640` creates an
`AgentOsSidecarClient` from `packages/core/src/sidecar/rpc-client.ts`.

That class, at `rpc-client.ts:1106-1448`, manufactures a second lifecycle:

- `AgentOsSidecarSessionState` and `AgentOsSidecarVmState` state machines;
- `AgentOsSidecarSessionLifecycle` and `AgentOsSidecarVmLifecycle` timestamped
  records;
- random session and VM IDs from `randomUUID`;
- a `sessions` map containing per-session VM maps;
- session and VM handle classes with `describe`, `listVms`, and `dispose`;
- transport bootstrap types carrying the invented IDs; and
- cloning/error bookkeeping used only by that synthetic state.

Production then adapts the real admin into that abstraction through
`createInProcessSidecarTransport` at `agent-os.ts:3642-3716`:

```text
synthetic session id
  -> synthetic VM id
    -> vmAdmins Map lookup by synthetic VM id
      -> real AgentOsVmAdmin
        -> real sidecar session id + real sidecar VM id
```

The synthetic IDs are not sent to the sidecar and are unrelated to
`nativeVm.vmId` or the real authenticated session. The production lease keeps
both synthetic handles only so `client.dispose()` can traverse its maps and
eventually call `admin.dispose()`.

The complete production sequence is currently:

```ts
const client = createAgentOsSidecarClient(...);
const session = await client.createSession(...); // invent UUID
const vm = await session.createVm();              // invent another UUID
const admin = transport.getVmAdmin(vm.vmId);      // recover real admin
```

This is client-owned runtime emulation, not transport forwarding or necessary
host callback state.

### The standalone tests prove only the manufactured model

`packages/core/tests/sidecar-client.test.ts` directly constructs the synthetic
class with fake transports. Its first test injects `id-1`/`id-2` and fake clock
ticks, then asserts the invented lifecycle records and maps. Its second test
creates multiple fake sessions/VMs and checks traversal order. Its third test
checks retry on the fake session transport.

Those tests do not cross the sidecar protocol and do not validate a real VM.
They preserve the abstraction Item 47 is meant to remove.

The unchanged before-characterization was executed on the inspected working
copy:

```text
pnpm --dir packages/core exec vitest run tests/sidecar-client.test.ts
Test Files  1 passed (1)
Tests       3 passed (3)
```

## Why direct leasing is the right boundary

The host still needs to know how many `AgentOs` instances lease one shared
child process, whether the handle is disposing, and when Node child/stdio
handles can be `ref()`/`unref()`ed. That is why these fields in
`AgentOsSidecarState` remain valid:

- `activeLeases` and the derived public `activeVmCount`;
- `nativeProcess` and `sharedChild`;
- `eventLoopHolds`;
- shared-pool identity; and
- the handle's ready/disposing/disposed state.

The sidecar owns actual session/VM IDs and runtime cleanup. The real admin owns
the minimum host adapters tied to that VM. No second lifecycle registry is
needed between them.

The Rust SDK already follows this conceptual boundary. Its
`AgentOsSidecarVmLease` in `crates/client/src/sidecar.rs:281-310` retains the
actual sidecar handle and decrements host lease accounting only after
authoritative shutdown. It has no synthetic session ID, VM ID, timestamped
lifecycle map, or transport adapter map.

## Exact production edits

### `packages/core/src/sidecar/rpc-client.ts`

Current exact anchors are `AgentOsSidecarPlacement` at lines 1102-1104; the
synthetic lifecycle block from `AgentOsSidecarSessionState` through
`createAgentOsSidecarClient` at lines 1106-1448; and its clone helpers at lines
1517-1549. `toErrorMessage` is at lines 1589-1591.

Delete the complete synthetic lifecycle surface:

- `AgentOsSidecarSessionState`;
- `AgentOsSidecarVmState`;
- `AgentOsSidecarSessionLifecycle`;
- `AgentOsSidecarVmLifecycle`;
- `AgentOsSidecarSessionOptions`;
- `AgentOsSidecarSessionBootstrap`;
- `AgentOsSidecarVmBootstrap`;
- `AgentOsSidecarTransport`;
- `AgentOsSidecarClientOptions`;
- `AgentOsSidecarVmEntry`;
- `AgentOsSidecarSessionEntry`;
- `AgentOsSidecarVmHandle`;
- `AgentOsSidecarSessionHandle`;
- `AgentOsSidecarClient`;
- `createAgentOsSidecarClient`;
- `clonePlacement`, `cloneSessionLifecycle`, and `cloneVmLifecycle`; and
- `toErrorMessage`, which is used only to populate the deleted fake lifecycle.

Remove the now-unused `randomUUID` import from this file. Keep `toError`: the
real `NativeSidecarKernelProxy` disposal path uses it.

Keep `AgentOsSidecarPlacement` for now because `agent-os.ts` uses it for the
public sidecar description/config shape. Moving that type is not required to
remove lifecycle behavior and would broaden the public declaration diff.

Do not touch the framed `SidecarProcess`, `NativeSidecarKernelProxy`, wire
serializers, or descriptor helpers in this file.

### `packages/core/src/agent-os.ts`

Current exact anchors are the synthetic imports at lines 140-150, the lease
shape at lines 181-202, the real `createVmAdmin` at lines 1411-1564, its lease
call at lines 1566-1572, legitimate host lease/process state at lines
3234-3269, the synthetic production lease at lines 3573-3669, and the admin-map
transport at lines 3671-3745.

1. Remove imports for `AgentOsSidecarClient`, both synthetic bootstrap types,
   both synthetic handle types, `AgentOsSidecarTransport`, and
   `createAgentOsSidecarClient`.

2. Rename the minimal local bound from `InProcessSidecarVmAdmin` to something
   that describes the retained real host resource, such as
   `DisposableSidecarVmAdmin`:

   ```ts
   interface DisposableSidecarVmAdmin {
     dispose(): Promise<void>;
   }
   ```

3. Simplify `AgentOsSidecarVmLease` by deleting `session` and `vm`:

   ```ts
   interface AgentOsSidecarVmLease<
     TVmAdmin extends DisposableSidecarVmAdmin,
   > {
     sidecar: AgentOsSidecar;
     admin: TVmAdmin;
     dispose(): Promise<void>;
   }
   ```

4. Delete `CreateInProcessSidecarTransportOptions`,
   `InProcessSidecarTransport`, and `createInProcessSidecarTransport` in full.
   Their only purpose is mapping synthetic IDs back to the real admin.

5. Change `leaseAgentOsSidecarVm` to accept the real admin factory directly:

   ```ts
   async function leaseAgentOsSidecarVm<
     TVmAdmin extends DisposableSidecarVmAdmin,
   >(
     sidecar: AgentOsSidecar,
     createVmAdmin: () => Promise<TVmAdmin>,
   ): Promise<AgentOsSidecarVmLease<TVmAdmin>>
   ```

6. Preserve the current ready-state check and acquire the shared-sidecar event
   loop hold before awaiting VM creation. Then call only:

   ```ts
   const admin = await createVmAdmin();
   ```

   There must be no client/session/VM UUID generation and no admin lookup.

7. Construct the lease around that `admin`. Its disposal sequence must remain:

   ```text
   await admin.dispose()
   mark lease disposed
   delete the lease record
   derive activeVmCount from activeLeases.size
   release the event-loop hold exactly once
   ```

   Keep the current shared `disposePromise`/retry structure. If
   `admin.dispose()` rejects, do not mark the lease disposed, remove it from
   `activeLeases`, decrement `activeVmCount`, or release the hold. Clear only
   the failed attempt promise so a later `AgentOs.dispose()` can retry.

8. On creation failure, release the event-loop hold exactly once and rethrow.
   `createVmAdmin` already owns rollback of a partially initialized real VM; do
   not manufacture another transport cleanup pass.

9. Change the call site at `AgentOs.create()` from:

   ```ts
   leaseAgentOsSidecarVm(sidecar, {
     createVm: async () => createVmAdmin(),
   })
   ```

   to:

   ```ts
   leaseAgentOsSidecarVm(sidecar, createVmAdmin)
   ```

Do not call `SidecarProcess.dispose()` from a VM lease. Each VM proxy has
`ownsClient: false`; the `AgentOsSidecar` handle owns the one shared process and
disposes it only after all real VM leases are gone.

## Proposed small diff sequence

Keep the implementation mechanically reviewable in this order:

1. Add the red lease-shape characterization to
   `packages/core/tests/sidecar-placement.test.ts` and record the parent failure:
   the lease has the authoritative `admin`, but also exposes synthetic `session`
   and `vm` fields. Run the unchanged synthetic unit suite and record its three
   passing tests as removal evidence.
2. In `agent-os.ts`, change the lease factory parameter from the adapter-options
   object to `createVmAdmin: () => Promise<TVmAdmin>`, call it directly, retain
   only `{ sidecar, admin, dispose }`, and make `dispose()` await
   `admin.dispose()` before changing the host lease set/count/hold.
3. Still in `agent-os.ts`, delete `CreateInProcessSidecarTransportOptions`,
   `InProcessSidecarTransport`, and `createInProcessSidecarTransport`, then
   remove their imports and the synthetic lease fields. Do not refactor the
   surrounding shared-sidecar pool or process lifecycle in this change.
4. In `rpc-client.ts`, delete the now-unreferenced manufactured lifecycle block,
   clone helpers, `toErrorMessage`, and its `randomUUID` import. Keep
   `AgentOsSidecarPlacement`, the real framed client/proxy, serializers, and
   `toError`.
5. Delete `sidecar-client.test.ts`, make the red real-lease test green, add the
   two-real-VM acceptance case, then run retry, sibling ownership, leak, and
   clean-exit coverage unchanged. Finish with typecheck and the zero-reference
   source inventory.

That sequence is one bounded Item 47 revision; the intermediate red test is
evidence to record, not a separate committed revision.

### No Rust, sidecar, protocol, or documentation edit

This is a TypeScript host-lifecycle deletion. No wire field or sidecar behavior
changes, and Rust already uses direct lease accounting. The public
`AgentOsSidecar` API (`describe`, `activeVmCount`, shared/explicit placement,
and `dispose`) remains unchanged.

`packages/core/CLAUDE.md:35` already states the correct boundary: framed I/O and
serializers live in `rpc-client.ts`, while shared/explicit pool and VM lease
bookkeeping live in `agent-os.ts`; neither layer owns runtime emulation or
policy. No guidance change is needed.

## Before validation

Before editing production code, add a focused regression to
`packages/core/tests/sidecar-placement.test.ts`, for example
`"retains only the authoritative VM admin in a host lease"`. Create a real VM
with an explicit sidecar ID, then use a typed test backdoor to inspect
`_sidecarLease`, `_sidecarVm`, and the lease admin:

```ts
const internals = vm as unknown as {
  _sidecarVm: CreatedVm;
  _sidecarLease: {
    admin: { sidecarVm: CreatedVm };
    session?: unknown;
    vm?: unknown;
  };
};

expect(internals._sidecarLease.admin.sidecarVm).toBe(internals._sidecarVm);
expect(internals._sidecarLease).not.toHaveProperty("session");
expect(internals._sidecarLease).not.toHaveProperty("vm");
```

The first assertion proves the lease already retains the authoritative VM. The
last two assertions fail on Item 47's parent because the production lease also
contains the manufactured `AgentOsSidecarSessionHandle` and
`AgentOsSidecarVmHandle`. Record that exact failure before changing the
implementation. This is intentionally an internal-boundary regression: the
synthetic lifecycle has no legitimate public behavior to exercise.

Also run `packages/core/tests/sidecar-client.test.ts` unchanged against Item
47's parent and record the revision and three passing test names in the
tracker. Those tests characterize exactly what is being removed:

- injected UUIDs become session/VM identities;
- fake timestamps and lifecycle states are retained in nested maps;
- client disposal walks those maps to reach a fake transport; and
- retry is implemented at the fake session layer.

Record the production source trace from `leaseAgentOsSidecarVm`: it creates
the class, calls synthetic `createSession`/`createVm`, and looks the real admin
back up by the invented VM ID. The standalone test alone does not prove
production usage; the red real-lease test plus this call-site trace does.

Do not retain the old fake-lifecycle assertions after the implementation. They
would test deleted behavior rather than compatibility.

## After validation

Delete `packages/core/tests/sidecar-client.test.ts` after its before evidence is
recorded. Keep the new red lease-shape regression and make it pass by retaining
only `sidecar`, the authoritative `admin`, and `dispose` on the lease. Expand
the lifecycle acceptance around real public behavior in
`packages/core/tests/sidecar-placement.test.ts`:

1. Create one explicit `AgentOsSidecar` and two real `AgentOs` VMs against it.
2. Assert `activeVmCount` changes `0 -> 1 -> 2` only as real VM creation
   succeeds.
3. Read each test VM's existing private `_sidecarVm.vmId` through a typed test
   backdoor and assert the authoritative IDs are non-empty and distinct. Do not
   add a production getter merely for this assertion.
4. Perform guest I/O in both VMs to prove both real admins remain live.
5. Dispose the first VM; assert the count becomes one and the sibling VM still
   performs guest I/O.
6. Dispose the first VM again; assert idempotence and no second decrement.
7. Dispose the second VM; assert the count becomes zero.
8. Dispose the sidecar and assert its state becomes `disposed`.

Retain these existing tests because each protects a subtle part of the direct
replacement:

- `packages/core/tests/agent-os-dispose-retry.test.ts` proves a rejected remote
  disposal retains the lease and host routes for retry;
- `packages/core/tests/shared-sidecar-ownership.test.ts` proves disposing one
  real VM does not destroy sibling VM host-tool or cron routing;
- `packages/core/tests/shared-sidecar-clean-exit.test.ts` proves the host child
  and stdio handles are unref'd after the last lease;
- the existing shared/explicit/non-default-pool cases in
  `sidecar-placement.test.ts` prove handle selection is unchanged; and
- `packages/core/tests/leak-rpc-client.test.ts` protects per-real-VM process
  route cleanup inside `NativeSidecarKernelProxy`.

Add a source-inventory assertion to the completion checklist rather than a
runtime compatibility shim. There should be no remaining references to the
deleted class, handles, lifecycle/bootstrap types, or adapter transport.

The primary after test should be named explicitly, for example
`"leases two authoritative VMs directly from one explicit sidecar"`, so the
tracker can cite one stable acceptance test rather than only a file-wide run.

## Validation commands

Build the actual sidecar and TypeScript package before the real lease tests:

```sh
cargo build -p agentos-sidecar
pnpm --dir packages/core build
AGENT_OS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/core exec vitest run \
    tests/sidecar-placement.test.ts \
    tests/shared-sidecar-ownership.test.ts \
    tests/agent-os-dispose-retry.test.ts \
    tests/leak-rpc-client.test.ts
AGENT_OS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/core exec vitest run \
    tests/shared-sidecar-clean-exit.test.ts
pnpm --dir packages/core check-types
git diff --check
```

Finish with this source inventory; it must return no matches:

```sh
rg -n \
  "AgentOsSidecarClient|AgentOsSidecarSessionHandle|AgentOsSidecarVmHandle|AgentOsSidecarSessionLifecycle|AgentOsSidecarVmLifecycle|AgentOsSidecarSessionBootstrap|AgentOsSidecarVmBootstrap|createInProcessSidecarTransport" \
  packages/core/src packages/core/tests
```

The focused test run is not valid if real sidecar tests skip because the binary
or built `dist` entry is missing.

## Risks and boundaries

- **Disposal retry:** remote VM disposal is authoritative. Never remove the
  lease or release its event-loop hold before `admin.dispose()` succeeds.
- **Sibling process ownership:** a VM lease must not dispose the shared
  `SidecarProcess` or close the shared authenticated session. The sidecar handle
  does that after every VM lease is gone.
- **Double disposal:** `AgentOs.dispose()`, `AgentOsSidecar.dispose()`, and retry
  paths can converge on one lease. Preserve the per-lease `disposePromise`,
  disposed flag, and one-shot hold release.
- **Creation failure:** the event-loop hold is acquired before asynchronous VM
  creation, so every failure path must release it exactly once. Do not add a
  synthetic failed-lifecycle tombstone.
- **Real identity:** host callback/event routing must continue using
  `sidecarSession.connectionId`, `sidecarSession.sessionId`, and
  `sidecarVm.vmId` from the real admin. No new local ID should replace them.
- **Session granularity:** TypeScript currently shares one real wire session
  across VMs on a sidecar handle while VM ownership is separated by real
  `vm_id`. Item 47 removes fake sessions but does not redesign that valid wire
  ownership model. Opening one real session per VM would require a separate
  transport/API decision and is not necessary for this deletion.
- **Create/dispose concurrency:** the current code registers a lease only after
  VM creation succeeds. The event-loop hold keeps Node handles referenced but
  is not itself a general lifecycle mutex. Do not claim Item 47 fixes a
  concurrent `sidecar.dispose()` versus in-flight `AgentOs.create()` race unless
  a dedicated test and a small explicit pending-lease mechanism are added. Do
  not recreate the deleted session/VM state machine to solve that separate
  concern.
- **Public API:** `AgentOsSidecarClient` is exported only from the repository-
  internal source module and is not in the package export map
  (`packages/core/package.json:11-41`) or root entrypoint. Remove it; do not
  deprecate or preserve it as public compatibility surface.

## Dependencies and nearby items

- Stack Item 47 after Item 46 as requested by the one-item revision workflow;
  there is no semantic dependency on Item 46's Rust presence work.
- Item 48 changes how a real VM admin materializes TypeScript host-backed overlay
  state after initialization. Land Item 47 first or rebase its `createVmAdmin`
  anchors carefully; do not pull Item 48's protocol/overlay changes into this
  revision.
- Item 52 removes a separate legacy ACP notification path. It may touch
  `agent-os.ts`, but it does not justify retaining the fake sidecar lifecycle.
- Item 65 tracks typed cleanup-error preservation. The manufactured client
  currently flattens failures with `errors.map(...).join("; ")`; deleting that
  layer lets the real admin's `AggregateError` propagate. Do not reproduce the
  flattened message behavior in the direct lease. Item 65 can still address
  other cleanup paths independently.
- Item 77 owns the broader create/dispose race and shared-process termination
  supervision. Preserve the existing hold and ready-state checks here, but do
  not expand Item 47 into that lifecycle redesign.
- No sidecar/protocol/Rust prerequisite exists. If implementation appears to
  require a new wire request, stop: that would indicate the direct-admin path
  has been bypassed rather than simplified.

## Dedicated Item 47 revision scope

Create a new stacked `jj` child only after Item 46 is sealed. Suggested
description:

```text
refactor(core): lease real sidecar VMs directly
```

Expected bounded path set:

- `packages/core/src/agent-os.ts`
- `packages/core/src/sidecar/rpc-client.ts`
- `packages/core/tests/sidecar-client.test.ts` (delete after recording before
  evidence)
- `packages/core/tests/sidecar-placement.test.ts`
- `docs/thin-client-migration.md` (checklists/status only after validation)

The retry, sibling-ownership, clean-exit, and leak tests should pass unchanged;
include them in validation but do not edit them unless the direct replacement
exposes a genuine expectation mismatch. No protocol, Rust, sidecar, runtime-core,
README, website, Cargo lock, or pnpm lockfile should change.

Before describing/sealing the revision, inspect `jj diff` and keep unrelated
shared-working-copy changes out of Item 47. Record the before characterization,
real after tests, validation commands, revision ID, and completion status in the
tracking row.
