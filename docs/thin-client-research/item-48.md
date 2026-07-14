# Item 48 research: resolve omitted overlay mode in the sidecar

Status: implementation-ready research only. Refreshed on **2026-07-14** against
the active Item 80 working-copy code shape (`pzzlonpr`, parent `suwmustu`). This note does not modify
production code, tests, or the Item 48 tracker status. Symbol names below are the
stable anchors because earlier stacked items can shift numeric line positions.

## Recommendation

Keep `OverlayMountConfig.filesystem.mode` optional at the public TypeScript API,
forward that omission unchanged, and have the sidecar return a resolved applied
view of the operator mount descriptors in both `VmConfiguredResponse` and
`VmInitializedResponse`. Materialize the TypeScript-only VFS/overlay bridge only
after `initialize_vm` succeeds, using the sidecar's required `readOnly` result to
choose a read-only or ephemeral host overlay.

Keep the original forwarded descriptor separately and reuse it unchanged for
later full-list mount reconfiguration. The response tells TypeScript how to
construct caller-owned host state; it does **not** authorize TypeScript to turn
an omitted `readOnly` into an explicit override on a later request.

The sidecar cannot own the `LayerStore`, `SnapshotLayerHandle`, or arbitrary
`VirtualFileSystem` object: those are live JavaScript objects reached through the
`js_bridge` callback. That backing state must remain in the TypeScript host. The
policy decision does not need to remain there. The correct split is:

```text
TypeScript caller input (mode may be omitted)
  -> MountDescriptor.readOnly remains omitted
  -> sidecar resolves omitted readOnly to its default
  -> response carries required resolved readOnly
  -> TypeScript builds only the corresponding host callback backing store
```

Add a distinct wire type rather than returning an input `MountDescriptor` whose
`readOnly` field is still optional:

```bare
type ResolvedMountDescriptor struct {
  guestPath: str
  readOnly: bool
  plugin: MountPluginDescriptor
}
```

Both response fields should be named `resolvedOperatorMounts`. They contain the
stored operator mount list only: the explicit request list when supplied, or the
retained operator list when a configure request omits the whole field. Order and
cardinality are unchanged and `readOnly` defaults are materialized. Guest path
and plugin data remain the submitted operator descriptor data; this item does
not add path canonicalization. The list deliberately excludes package/provides
mounts owned and added by the sidecar.

Priority: **P2**. Confidence: **medium**, matching the tracker. The diagnosis
and ownership boundary are high confidence, and native mount application already
uses the correct sidecar default. Implementation confidence is medium because
this is a lockstep protocol addition across native, browser, runtime-core, and
generated bindings, and TypeScript must preserve exact
descriptor-to-host-object correlation while Item 47 reshapes VM leasing.

## Original issue

The tracker entry is at `docs/thin-client-migration.md:94,181,273` in the
inspected working copy.

`resolveCompatLocalMounts` in `packages/core/src/agent-os.ts` currently resolves
JavaScript-backed mounts before a sidecar VM exists. For an overlay it selects a
runtime policy default itself:

```ts
const mode = mount.filesystem.mode ?? "ephemeral";
const fs =
	mode === "read-only"
		? mount.filesystem.store.createOverlayFilesystem({
				mode: "read-only",
				lowers: mount.filesystem.lowers,
			})
		: mount.filesystem.store.createOverlayFilesystem({
				upper: await mount.filesystem.store.createWritableLayer(),
				lowers: mount.filesystem.lowers,
			});
```

That helper is invoked near the start of `AgentOs.create`, before
`createVmAdmin`, `ensureSharedSidecarNativeProcess`, and `initializeVm`. It
therefore allocates a writable layer and commits to ephemeral behavior before a
sidecar process has even been acquired, much less accepted the VM or resolved
the mount.

At the same time, `collectSidecarMountPlan` correctly preserves the omission on
the wire:

```ts
const readOnly = isOverlayMountConfig(mount)
	? mount.filesystem.mode === undefined
		? undefined
		: mount.filesystem.mode === "read-only"
	: mount.readOnly;
```

The result is two policy paths for the same mount:

- the sidecar applies `MountDescriptor::effective_read_only()` from
  `crates/sidecar-protocol/src/wire.rs:65-69`, where omission currently resolves
  to writable (`false`);
- TypeScript independently interprets omission as ephemeral, allocates the
  writable upper, and stores an omitted `LocalCompatMount.readOnly` value.

Those happen to agree today. They can diverge as soon as the sidecar changes or
contextualizes its default. Dynamic mount reconfiguration in
`NativeSidecarKernelProxy.desiredSidecarMounts` correctly needs to resend the
caller's omission unchanged; the bug is that the already-created host overlay
was chosen without consuming the sidecar's applied result. The fix therefore
needs two values: the original descriptor for future requests and the resolved
descriptor for constructing host-only backing state.

The local default appears a second time in
`packages/core/src/overlay-filesystem.ts`, and the `LayerStore` API permits an
omitted ephemeral mode on its writable branch. The existing test named
`defaults upper to in-memory filesystem` in
`packages/core/tests/overlay-backend.test.ts` explicitly preserves that default
by constructing `createOverlayBackend({ lower })` and expecting an implicit
writable in-memory upper. This is part of the tracker's “before” behavior.

## Exact protocol edit

### `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare`

Add `ResolvedMountDescriptor` immediately after `MountDescriptor`. Add this
required field to both response types:

```bare
type VmConfiguredResponse struct {
  appliedMounts: u32
  resolvedOperatorMounts: list<ResolvedMountDescriptor>
  projectedCommands: list<ProjectedCommand>
  agents: list<AgentosProjectedAgent>
}

type VmInitializedResponse struct {
  vmId: str
  guestCwd: str
  guestEnv: map<str><str>
  processRouteRetention: u64
  appliedMounts: u32
  resolvedOperatorMounts: list<ResolvedMountDescriptor>
  projectedCommands: list<ProjectedCommand>
  agents: list<AgentosProjectedAgent>
  hostCallbacks: list<HostCallbacksRegisteredResponse>
}
```

Document on the field that it is a resolved applied view of the operator mount
list, not the full internal applied mount list. When
`ConfigureVmRequest.mounts` is omitted, it describes the VM's retained operator
mounts. Every returned `readOnly` is concrete; guest path and plugin are copied
without claiming that this response canonicalizes them.

Run `pnpm --dir packages/build-tools build:protocol` and commit the regenerated
`packages/runtime-core/src/generated-protocol.ts`. Rust generated types are
built from the schema by `crates/sidecar-protocol/build.rs`; there is no
committed generated Rust file to hand-edit. Inspect the generated diff directly:
the declared `packages/build-tools` `check:generated` script currently targets
the missing root file `scripts/check-generated-artifacts.mjs`, so it is a known
repository gate defect rather than usable Item 48 validation.

In `crates/sidecar-protocol/src/protocol.rs`, add the compatibility alias
`pub type ResolvedMountDescriptor = crate::wire::ResolvedMountDescriptor;`
beside the existing `MountDescriptor` alias. This lets shared response helpers
use the generated type through the same `protocol` surface as every other wire
descriptor; do not create a second handwritten struct.

No backwards-compatibility shim or optional response field is needed. Protocol,
sidecars, and clients ship in lockstep.

## What remains client-side

Only state that the sidecar cannot serialize or access stays in TypeScript:

- the caller's live `VirtualFileSystem`, `LayerStore`, snapshot handles, and
  writable-layer handles;
- the `js_bridge` callback routing that lets a sidecar mount call those live
  JavaScript objects;
- correlation between one forwarded `js_bridge` descriptor and its caller-owned
  object, plus cleanup of host objects when startup or reconfiguration fails; and
- TypeScript package-manager default packages, which are the documented thin-
  client exception and are unrelated to overlay policy.

This does not leave a policy decision in the client. TypeScript validates the
sidecar's returned descriptor and constructs exactly the host backing it names.
The current `js_bridge` mount plugin does not invoke the host while the mount is
opened, so `initialize_vm` can safely commit the mount before TypeScript creates
the callback backing. Calls begin only after the returned VM is exposed through
`NativeSidecarKernelProxy`.

Rust needs no equivalent host object. It should decode the lockstep response but
must not cache, reinterpret, or add a public overlay result solely for parity.

## Exact sidecar edits

### `crates/native-sidecar-core/src/frames.rs`

Import `MountDescriptor` and `ResolvedMountDescriptor`. Add one shared resolver
so native and browser cannot materialize the default independently:

```rust
fn resolved_operator_mounts(mounts: &[MountDescriptor]) -> Vec<ResolvedMountDescriptor> {
    mounts
        .iter()
        .map(|mount| ResolvedMountDescriptor {
            guest_path: mount.guest_path.clone(),
            read_only: mount.effective_read_only(),
            plugin: mount.plugin.clone(),
        })
        .collect()
}
```

Change `vm_configured_response` at `frames.rs:178-191` to accept
`operator_mounts: &[MountDescriptor]` and fill `resolved_operator_mounts` with
that helper. Do not make callers calculate booleans themselves.

Update `shared_response_helpers_preserve_payloads` at `frames.rs:648-662` to
pass `&[]`. Add a focused unit assertion with one `MountDescriptor` whose
`read_only` is `None` and one whose value is `Some(true)`; the response must
contain required `false` and `true` respectively while preserving order,
guest path, plugin ID, and plugin config.

### `crates/native-sidecar/src/vm.rs`

At the successful configure response at `vm.rs:655-659`, pass
`&operator_mounts` into `vm_configured_response`. This variable is the correct
source:

- it is the explicit request list when supplied;
- it is the stored operator list when the request omits mounts;
- it excludes sidecar-owned package and `provides` mounts that are appended only
  to `effective_mounts`; and
- it is the same list whose `effective_read_only()` values were used by mount
  reconciliation through the mount helpers, which call
  `MountDescriptor::effective_read_only()`.

Do not resolve and store the descriptors back into
`VmConfiguration.operator_mounts`. Preserving caller omission in stored config
allows a future sidecar-default change to apply consistently on a later
reconfiguration. Resolve only the applied/result view.

### `crates/native-sidecar-browser/src/wire_dispatch.rs`

The browser dispatcher currently rejects any non-empty host mount list at
`wire_dispatch.rs:738-744`. Retain that capability boundary. After the rejection
check, bind `let operator_mounts = mounts.unwrap_or_default()` and pass
`&operator_mounts` to the shared `vm_configured_response` call at
`wire_dispatch.rs:808-812`. It is empty today, but returning the field keeps the
browser and native protocol behavior identical and avoids a second response
shape.

### Atomic initialization responses

Copy the already-resolved configure result into initialization:

- add `resolved_operator_mounts: configured.resolved_operator_mounts` to
  `crates/native-sidecar/src/service.rs:1601-1613`;
- make the same addition at
  `crates/native-sidecar-browser/src/wire_dispatch.rs:1763-1776`.

This is preferable to recomputing during initialization: `initialize_vm` is an
atomic composition of the create/configure/register operations, and its final
response should expose the exact configure result it committed.

## Exact runtime-core edits

### Generated/live descriptor mapping

In `packages/runtime-core/src/descriptors.ts`, add:

```ts
export interface LiveResolvedMountDescriptor {
	guest_path: string;
	read_only: boolean;
	plugin: NativeMountPluginDescriptor;
}
```

Add `fromGeneratedResolvedMountDescriptor`, parsing optional plugin `JsonUtf8`
config with the existing JSON utility and preserving absence. Keep input
`LiveMountDescriptor.read_only` optional; only the response type is concrete.

In `packages/runtime-core/src/response-payloads.ts`:

- add `resolved_operator_mounts: LiveResolvedMountDescriptor[]` to the
  `vm_initialized` and `vm_configured` variants at lines 147-165;
- map `payload.val.resolvedOperatorMounts` in both generated-response cases at
  lines 367-405.

### High-level sidecar process result

In `packages/runtime-core/src/sidecar-process.ts`, add a concrete result type:

```ts
export interface SidecarResolvedMountDescriptor
	extends SidecarMountDescriptor {
	readOnly: boolean;
}
```

Add `resolvedOperatorMounts: SidecarResolvedMountDescriptor[]` to
`InitializedVm` at lines 243-251 and `SidecarVmConfiguredResponse` at lines
360-364. Map the live snake-case descriptors in both `initializeVm` at lines
578-601 and `configureVm` at lines 675-685. A small
`fromWireResolvedMountDescriptor` helper should clone the plugin config just as
the existing request mapper does; do not reuse or mutate the caller's original
descriptor objects.

The Rust SDK at `crates/client/src/agent_os.rs:309-354` may ignore the additional
initialization field. Rust has no JavaScript callback-backed `LayerStore` to
materialize, so inventing a Rust client cache or public result merely for parity
would add non-essential state. It still decodes the same lockstep response.

## Exact TypeScript core edit

### Preserve correlation in `collectSidecarMountPlan`

Do not correlate the result back to live JavaScript objects by guest path alone:
paths may be normalized, and native descriptor deduplication already exists.
Extend the plan with an explicit response index recorded when each `js_bridge`
descriptor is pushed:

```ts
interface CompatLocalMountPlan {
	mount: PlainMountConfig | OverlayMountConfig;
	sidecarMountIndex: number;
	forwardedDescriptor: SidecarMountDescriptor;
}
```

Change `collectSidecarMountPlan` to return both `sidecarMounts` and
`compatLocalMounts`. For each non-native input, construct one `js_bridge`
descriptor, record `sidecarMounts.length`, and retain that exact descriptor in
the local plan before pushing it. Native mounts continue through their existing
dedupe path and do not get a local plan.

Keep `OverlayMountConfig.filesystem.mode` and both corresponding Zod schema
fields optional. That omission is the public input that must reach the sidecar.

### Resolve host backing state only after the response

Change `resolveCompatLocalMounts` to accept the deferred plans and
`nativeVm.resolvedOperatorMounts`. For every plan:

1. fetch the descriptor at `sidecarMountIndex`;
2. fail with a typed/descriptive startup error if it is absent, its normalized
   guest path differs, or its plugin ID is not `js_bridge`;
3. use the required `descriptor.readOnly`, never `?? "ephemeral"` or the
   original omitted value;
4. for a plain VFS, retain the caller's driver; no local policy wrapper is
   needed because the sidecar/kernel enforces the resolved mount mode;
5. for an overlay, call `createOverlayFilesystem({ mode: "read-only", ... })`
   when true; otherwise create the writable layer and call
   `createOverlayFilesystem({ mode: "ephemeral", upper, ... })`;
6. store `forwardedDescriptor` as `LocalCompatMount.sidecarMount`. Later
   `desiredSidecarMounts()` must resend the exact caller-authored descriptor,
   including an omitted `readOnly`; it must not manufacture an explicit boolean
   from the response.

Delete the eager call near the start of `AgentOs.create` and materialize the
deferred plans immediately after the `initializeVm` result. Keep
`createdNativeVm = nativeVm` before validation or host materialization. If the
response is malformed or layer creation fails, the existing catch path can then
dispose the committed sidecar VM. This ordering also proves that a rejected
initialization cannot allocate a local writable layer.

Construct `NativeSidecarKernelProxy` only after local resolution succeeds, with
the resolved local mounts and the original forwarded `sidecarMounts`.

### Remove the remaining local overlay-mode defaults

The host overlay engine should receive an explicit mode from its caller after
this change:

- in `packages/core/src/overlay-filesystem.ts:17-49`, make
  `OverlayBackendOptions.mode` required and replace
  `const mode = options.mode ?? "ephemeral"` with `const mode = options.mode`;
- in `packages/core/src/layers.ts:53-63`, require `mode: "ephemeral"` on the
  writable union branch;
- at `layers.ts:315-324`, pass `mode: "ephemeral"` explicitly into
  `createOverlayBackend`;
- update direct core test construction to state `mode: "ephemeral"` explicitly.

Do not remove the ability for explicit ephemeral mode to create a fresh
in-memory upper when the low-level caller omitted `upper`. The problem is the
mode default, not the copy-on-write implementation.

## Before and after tests

### Before evidence recorded by this research

- [x] `packages/core/tests/overlay-backend.test.ts:173-180`, currently named
  `defaults upper to in-memory filesystem`, proves the local backend selects
  ephemeral behavior when mode is omitted.
- [x] `packages/core/tests/mount.test.ts:229-307` proves the current public omitted
  overlay is writable and an explicit read-only overlay rejects writes. This is
  the user-visible behavior that should remain under today's sidecar default.
- [x] Source assertion: `AgentOs.create` awaits `resolveCompatLocalMounts` before
  it defines/runs `createVmAdmin`, which later calls
  `ensureSharedSidecarNativeProcess` and `initializeVm`. The “before” checklist
  should record this exact sequencing; a passing behavior test alone cannot
  distinguish two identical defaults.

The exact **parent-failing before test** should be added as
`packages/core/tests/overlay-sidecar-resolution.test.ts` before changing the
implementation:

1. use `defaultSoftware: false` and a unique explicit `AgentOsSidecar`;
2. spy on `SidecarProcess.spawn` and return a minimal cast fake whose
   authentication succeeds but whose `initializeVm` rejects with a sentinel
   sidecar error;
3. pass an overlay mount with omitted `mode` and a `LayerStore` whose
   `createWritableLayer` is spied;
4. assert `AgentOs.create(...)` rejects with the sentinel error; and
5. assert `createWritableLayer` was **not** called.

That last assertion fails against the parent because
`resolveCompatLocalMounts(options.mounts)` allocates the upper before
`SidecarProcess.spawn`/`initializeVm`. It passes after materialization moves
behind the successful resolved response. The fake must implement
`closeSession`/`dispose`, the test must restore the static spawn spy, and the
unique sidecar must be disposed in `afterEach` so shared global state does not
leak.

### After coverage to add or update

- [ ] **Shared sidecar normalization —** extend
   `crates/native-sidecar-core/src/frames.rs` unit coverage with omitted,
   explicit false, and explicit true descriptors. Assert required concrete
   results and descriptor ordering.
- [ ] **Native initialization —** add a case in
   `crates/native-sidecar/tests/initialize_vm.rs` whose initialization request
   contains a supported mount with `read_only: None`; assert
   `resolved_operator_mounts[0].read_only == false`. Add one explicit true mount
   assertion or cover that branch in the shared-core test.
- [ ] **Native configure retention —** extend a focused configure test in
   `crates/native-sidecar/tests/service.rs` to assert both an explicit request
   and a subsequent request with `mounts: None` return the same resolved
   operator descriptors. This proves omission of the whole list retains state
   while omission of an individual boolean is resolved by the sidecar.
- [ ] **Browser parity —** update
   `browser_wire_dispatcher_configures_vm_permissions` and
   `browser_wire_dispatcher_initializes_vm_atomically` in
   `crates/native-sidecar-browser/tests/wire_dispatch.rs` to assert an empty
   `resolved_operator_mounts` list. Keep the existing non-empty host-mount
   rejection coverage.
- [ ] **Generated protocol parity —** add the new field to
   `crates/native-sidecar/tests/generated_protocol.rs` and
   `packages/core/tests/generated-protocol.test.ts` fixtures. Use at least one
   descriptor in one fixture so required `readOnly` mapping is exercised, not
   only an empty list.
- [ ] **runtime-core mapping —** update
   `packages/runtime-core/tests/response-payloads.test.ts` and the in-memory
   initialization response in
   `packages/runtime-core/tests/sidecar-process.test.ts`. Assert the generated
   camel-case value becomes live snake case and then high-level camel case with
   a required boolean. Keep the existing request test proving omission is sent
   as `null`/absent.
- [ ] **Typed fixture completeness —** add `resolved_operator_mounts: []` to the
   live `vm_initialized` fixtures in
   `packages/runtime-browser/tests/runtime/converged-executor-session.test.ts`
   and any surviving BARE capture fixture in
   `packages/core/tests/native-sidecar-process-permissions.test.ts` after Item
   45. A repository-wide `rg` for `vm_initialized`, `vm_configured`,
   `VmInitializedResponse {`, and `VmConfiguredResponse {` must leave no
   required-field construction stale.
- [ ] **TypeScript mount behavior —** in `packages/core/tests/mount.test.ts`, cover
   three cases: omitted mode follows today's sidecar writable result, explicit
   `ephemeral` remains writable/isolated, and explicit `read-only` returns
   `EROFS`. Retain the existing lower-layer isolation assertion.
- [ ] **Resolved response controls host materialization —** in
   `overlay-sidecar-resolution.test.ts`, have the fake sidecar return
   `readOnly: true` for an input descriptor whose mode/read-only field was
   omitted. Assert `createWritableLayer` is not called and
   `createOverlayFilesystem` receives `{ mode: "read-only", ... }`. In a second
   case return `readOnly: false`; assert one writable layer is allocated and
   `createOverlayFilesystem` receives explicit `mode: "ephemeral"`. Also inspect
   the captured initialization request and a later reconfiguration request to
   prove both retain the omitted field. This is the decisive regression test:
   the true-response case fails on the parent because the host overlay has
   already been fixed to ephemeral before any response exists.
- [ ] **No low-level default —** replace the test at
   `overlay-backend.test.ts:173-180` with an explicit-ephemeral test. Add
   `mode: "ephemeral"` to the remaining direct constructors in that file and to
   writable `createOverlayFilesystem` calls in `layers.test.ts` and
   `leak-layer-store.test.ts`.
- [ ] **Rejected-sidecar allocation guard —** keep the parent-failing
   `overlay-sidecar-resolution.test.ts` case above and make it pass. This
   validates sequencing at runtime instead of relying only on a source
   assertion.
- [ ] **Malformed-response rollback —** return a missing or mismatched
   `resolvedOperatorMounts` entry from the same fake-sidecar test file. Assert a
   descriptive startup failure, zero writable-layer allocations before response
   validation, and one `disposeVm` call for the already-created VM. Add a
   materialization-failure variant in the same harness to prove rollback after
   allocation begins. Restore the static spawn spy and dispose the unique
   sidecar in `afterEach` so the process-global pool cannot leak between tests.
- [ ] **Caller-owned bridge state —** run
   `packages/core/tests/custom-vfs-mount-hook.test.ts` unchanged. It proves the
   sidecar still routes VFS callbacks to the caller-owned JavaScript object; the
   protocol response must not attempt to serialize or move that object.

Focused validation commands:

```sh
cargo test -p agentos-native-sidecar-core
cargo test -p agentos-native-sidecar --test initialize_vm --test generated_protocol
cargo test -p agentos-native-sidecar-browser --test wire_dispatch
pnpm --dir packages/runtime-core test -- response-payloads.test.ts sidecar-process.test.ts
pnpm --dir packages/runtime-core check-types
pnpm --dir packages/runtime-browser test -- converged-executor-session.test.ts
pnpm --dir packages/runtime-browser check-types
pnpm --dir packages/core test -- overlay-backend.test.ts layers.test.ts leak-layer-store.test.ts mount.test.ts overlay-sidecar-resolution.test.ts custom-vfs-mount-hook.test.ts generated-protocol.test.ts
pnpm --dir packages/core check-types
cargo check --workspace
git diff --check
```

Run protocol generation/consistency before the focused suites:

```sh
pnpm --dir packages/build-tools build:protocol
jj diff -- packages/runtime-core/src/generated-protocol.ts
cargo fmt --all -- --check
```

Until `scripts/check-generated-artifacts.mjs` is restored, record the known
`check:generated` failure in the revision validation rather than treating it as
an Item 48 regression.

## Dependencies and sequencing

- Implement this as the next one-item revision after Item 47. Item 47 is still
  marked pending at `docs/thin-client-migration.md:174` and changes the
  TypeScript VM-admin construction seam. The protocol, native sidecar, browser,
  and runtime-core portions can be prepared independently, but the final
  `agent-os.ts` edit must target Item 47's resulting direct-admin path.
- The schema change is lockstep by project policy. Do not add an optional field,
  capability negotiation, or a compatibility fallback in either client.
- Browser support for caller-owned host mounts is out of scope. Returning an
  empty resolved list is parity for the browser's existing supported input.
- Item 48 must not absorb package projection, `js_bridge` protocol redesign, or
  public `LayerStore` cleanup APIs. If partial overlay construction reveals a
  real resource-release gap, record it as a separate bounded issue rather than
  hiding the original startup error.

## Risks and guards

- **Wrong descriptor correlation:** never search only by path. Carry the exact
  sidecar list index in the deferred local plan and validate normalized path,
  plugin ID, and expected plugin config on the response before attaching a live
  object. A mismatch is a protocol error, never a cue to guess.
- **Returning internal package mounts:** use `operator_mounts`, not
  `effective_mounts`; otherwise response size and ordering depend on package
  projection internals and cannot map to client objects.
- **Reintroducing a client fallback:** the resolved response type must require a
  boolean. Missing/mismatched response data is a protocol/startup error, not a
  reason to fall back to ephemeral.
- **Leaking a committed VM:** assign `createdNativeVm` before host overlay
  materialization so the existing startup rollback reaches `disposeVm`.
- **Leaking a host layer on partial local resolution:** if multiple local mounts
  are materialized and a later one fails, dispose/release any already-created
  host overlay resources if the layer abstraction gains such an API. Today the
  `LayerStore` has no per-overlay release operation, so do not invent a silent
  cleanup; preserve the original error and log any available cleanup failure.
- **Dynamic reconfiguration regression:** store the original forwarded
  descriptor on `LocalCompatMount` and resend it unchanged. Do not store the
  resolved boolean in the request descriptor: doing so would turn the
  sidecar's observation into a client-authored override and violate the thin-
  client omission invariant.
- **Browser overreach:** do not add browser host-mount support in this item. It
  returns an empty resolved list because it accepts only absent/empty mounts.
- **Accidental public narrowing:** only low-level host-overlay construction gets
  a required mode. Public `AgentOs.create({ mounts: [{ filesystem: ... }] })`
  must continue accepting an omitted mode and forwarding it.

## Bounded dedicated JJ revision

Implement Item 48 in one new stacked `jj` revision, separate from every other
tracker item. The expected revision path set is bounded to:

```text
crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare
crates/sidecar-protocol/src/protocol.rs
crates/native-sidecar-core/src/frames.rs
crates/native-sidecar/src/vm.rs
crates/native-sidecar/src/service.rs
crates/native-sidecar/tests/initialize_vm.rs
crates/native-sidecar/tests/service.rs
crates/native-sidecar/tests/generated_protocol.rs
crates/native-sidecar-browser/src/wire_dispatch.rs
crates/native-sidecar-browser/tests/wire_dispatch.rs
packages/runtime-core/src/generated-protocol.ts
packages/runtime-core/src/descriptors.ts
packages/runtime-core/src/response-payloads.ts
packages/runtime-core/src/sidecar-process.ts
packages/runtime-core/tests/response-payloads.test.ts
packages/runtime-core/tests/sidecar-process.test.ts
packages/runtime-browser/tests/runtime/converged-executor-session.test.ts
packages/core/src/agent-os.ts
packages/core/src/layers.ts
packages/core/src/overlay-filesystem.ts
packages/core/tests/generated-protocol.test.ts
packages/core/tests/overlay-backend.test.ts
packages/core/tests/layers.test.ts
packages/core/tests/leak-layer-store.test.ts
packages/core/tests/mount.test.ts
packages/core/tests/overlay-sidecar-resolution.test.ts
packages/core/tests/native-sidecar-process-permissions.test.ts
docs/thin-client-migration.md
```

`packages/core/src/options-schema.ts` and
`packages/core/tests/custom-vfs-mount-hook.test.ts` are verification-only unless
a failing test reveals a real mismatch.
`packages/core/tests/native-sidecar-process-permissions.test.ts` is needed only
if Item 45's replacement BARE fixture still constructs `vm_configured` locally;
update that fixture rather than reviving JSON compatibility. Do not include
unrelated generated files, package dependencies, Rust client state, package
projection code, or browser mount capability work. Mark the Item 48 tracker
checkboxes complete only after the before evidence is recorded, all after tests
pass, and the dedicated revision ID is written into the tracking row.
