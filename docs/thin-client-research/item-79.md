# Item 79 research: trusted VM teardown must ignore guest filesystem policy

## Verdict

- **Priority:** P1
- **Fix confidence:** High
- **Root-cause confidence:** High
- **Owning layer:** kernel/native sidecar, with browser parity coverage
- **Client change required:** none

The failure is caused by native sidecar mount teardown using the guest-facing
kernel unmount API. `KernelVm::unmount_filesystem()` deliberately asks the VM's
filesystem permission callback for `FsOperation::Write`; therefore a guest
policy that denies `fs.write` on the mounted path also denies the sidecar's own
disposal unmount. Trusted lifecycle cleanup must use a narrowly named operator
path that bypasses the guest permission callback while retaining normal VFS
validation and errors.

## Exact failure chain

1. `AgentOs.create()` configures the native VM with the requested permissions
   and the TypeScript test's `nodeModulesMount(...)` at `/root/node_modules`.
   Native creation installs the dynamic permission callback from
   `bridge_permissions(...)` in
   `crates/native-sidecar/src/bridge.rs`.
2. `AgentOs.dispose()` reaches
   `packages/core/src/sidecar/rpc-client.ts::SidecarRpcProxy.disposeOnce()`,
   which calls the runtime client's `disposeVm(...)` protocol operation.
3. `crates/native-sidecar/src/vm.rs::dispose_vm_internal_outcome_with_event_limit()`
   calls `shutdown_configured_mounts(..., "dispose_vm", true, false)`.
4. `shutdown_configured_mounts()` calls
   `vm.kernel.unmount_filesystem(&existing.guest_path)` for every configured
   mount.
5. `crates/kernel/src/kernel.rs::KernelVm<MountTable>::unmount_filesystem()`
   calls `check_mount_permissions()`. That performs
   `self.filesystem.check_path(FsOperation::Write, path)` (and an additional
   `MountSensitive` probe for sensitive paths).
6. The permission bridge maps `FsOperation::Write` to guest `fs.write`.
   Denying writes at `/`/`/**` therefore returns `EACCES` while attempting to
   detach `/root/node_modules`.
7. Native teardown continues and reclaims the VM, but retains the unmount error
   in `mount_result`; the disposal response is consequently a
   `cleanup_failed` rejection. The `continue_on_error` comment immediately
   above this call is stale: the helper does continue through all mounts, but
   returns `Err(SidecarError::Cleanup { ... })` when any failed.
8. The two TypeScript messages are one server failure seen twice. The first
   rejected admin disposal becomes `failed to dispose sidecar VM`; because the
   admin remains in the session transport map, session disposal retries that
   admin and wraps it as `failed to dispose sidecar session`.

Removing `rm` from the denied operations cannot help because unmount checks
`fs.write`, not `fs.rm`. `create_dir` is also unrelated to the teardown failure.
The compiler request itself succeeds because Item 42 now sends its payload over
stdin and does not need a transport directory.

## Recommended implementation

### 1. Add an explicit trusted unmount seam in the kernel

Edit `crates/kernel/src/kernel.rs`, in `impl KernelVm<MountTable>` next to the
existing mount/unmount methods:

- Keep `unmount_filesystem()` unchanged as the guest/in-band operation. It must
  continue to call `check_mount_permissions()` so executor-originated unmounts
  remain denied.
- Add a narrowly named public internal-runtime API such as
  `unmount_filesystem_for_operator(&mut self, path: &str)`. It should:
  1. call `assert_not_terminated()`;
  2. call the underlying
     `self.filesystem.inner_mut().inner_mut().unmount(path)` directly;
  3. map `VfsError` to `KernelError` exactly like the existing method.
- Document that this is only for trusted sidecar/runtime configuration and
  teardown, never executor requests. Do not implement it by temporarily
  replacing the VM policy with allow-all: process-termination failure can leave
  guest work alive during later teardown phases, creating a policy race.

The lower `MountTable::unmount()` in
`crates/vfs/src/posix/mount_table.rs` still normalizes the path, rejects root,
rejects busy parent mounts, and reports missing mounts. The new seam bypasses
only the guest authorization callback, not VFS correctness checks.

### 2. Use the trusted seam for native sidecar-owned mounts

Edit `crates/native-sidecar/src/vm.rs`:

- In `shutdown_configured_mounts()`, replace
  `vm.kernel.unmount_filesystem(&existing.guest_path)` with the operator API.
  This helper is reached from both disposal and `ConfigureVm` reconciliation;
  both are trusted client/sidecar operations under the repository trust model,
  so neither should depend on executor permissions.
- Correct the stale comment in
  `dispose_vm_internal_outcome_with_event_limit()`: `continue_on_error` means
  "attempt every mount", not "the helper cannot return Err". Preserve genuine
  unmount/plugin/audit errors in the aggregated disposal failure; do not silence
  them.

No TypeScript fallback, permission override, bootstrap mkdir, or client cleanup
logic should be added. The client already forwards disposal correctly.

### 3. Converge the browser's existing trusted bypass on the same API

Edit `crates/native-sidecar-browser/src/service.rs`:

- `rollback_projected_package_mounts()` currently reaches through three layers
  (`filesystem_mut().inner_mut().inner_mut().unmount(path)`) to perform the same
  trusted bypass. Replace that with `unmount_filesystem_for_operator(path)`.
- Browser VM disposal itself removes/drops the kernel and is already independent
  of guest filesystem policy; no new guest-policy mutation is needed there.
  Add lifecycle coverage so native/browser parity is explicit and future
  cleanup changes cannot reintroduce the dependency.

An optional follow-up, not required to close this item, is a symmetric
operator-mount API for `mount_leaf_descriptors()`. Native configure currently
temporarily installs allow-all while doing broader sidecar-owned discovery and
mount reconciliation. Removing that wider policy swap needs its own audit; do
not expand Item 79 into that refactor.

## Before tests

The tracked real TypeScript reproduction is sufficient before-proof:

- In
  `packages/typescript/tests/typescript-tools.integration.test.ts`, configure
  the stdin compiler VM with `nodeModulesMount(...)`, allow reads, and deny
  `write` plus `create_dir` at `/`/`/**`.
- Compilation returns the expected TypeScript diagnostic.
- Pre-fix, the `finally { await restrictedVm.dispose(); }` call rejects with the
  nested VM/session disposal messages. The sidecar error should include the
  configured mount path and an `EACCES`/permission-denied unmount failure.

For the smallest Rust before-proof, extend the existing kernel permission test
in `crates/kernel/tests/permissions.rs`: a mounted `/workspace` with a denying
permission callback makes `unmount_filesystem("/workspace")` return `EACCES`.
That behavior is correct and must remain; the bug is calling this guest API from
trusted teardown.

## After tests

### Kernel boundary test

In `crates/kernel/tests/permissions.rs`, beside
`kernel_unmounts_require_write_permission_on_the_mount_path`:

- seed a mount under a filesystem callback that denies and records every probe;
- assert guest `unmount_filesystem()` remains `EACCES` and the mount remains;
- call `unmount_filesystem_for_operator()` and assert it succeeds;
- assert the trusted call did not add a permission probe and the mount is gone.

This test proves the bypass is narrowly scoped instead of weakening guest
enforcement.

### Native lifecycle test

Add a focused test to `crates/native-sidecar/tests/filesystem.rs` (the existing
`shadow_root` module already has host-mount creation, guest filesystem request,
disposal, and session helpers), or to `permission_flags.rs` if kept standalone:

1. Create a VM and configure a real `host_dir` mount.
2. Apply a filesystem policy that permits the read needed by the assertion but
   denies `write` and `create_dir` for `/`/`/**`.
3. Assert a guest write is still rejected with `guest_filesystem_failed` and
   `EACCES`.
4. Dispatch `DisposeVmRequest` and assert an actual
   `VmDisposedResponse` (do not merely assert that dispatch returned `Ok`, since
   protocol rejection is also an `Ok(DispatchResult)`).
5. Close the session and connection successfully and assert the sidecar debug
   counts show zero VMs/disposal progress/routes. If bridge permission-map
   introspection is exposed, also assert the VM policy entry was cleared.

Cover both direct `DisposeVmRequest` and session-owned cleanup, either in one
test with two VMs or two small cases, because the protocol has both teardown
entry points.

### Browser lifecycle parity test

In `crates/native-sidecar-browser/tests/wire_dispatch.rs`, reuse the existing
deny-all configure and close-session helpers:

1. Create/bootstrap the browser VM, optionally project the packed browser agent
   fixture so mounted package state is present, then configure deny-all.
2. Prove a guest write/read covered by the policy is rejected.
3. Dispose the VM or close its session and assert `VmDisposedResponse` or
   `SessionClosedResponse`, `dispatcher.vm_count() == 0`, and sidecar context,
   worker, and pending-cleanup counts are zero.

The browser test is parity protection. Its current disposal path is already
trusted; the implementation change there is replacing the package rollback's
deep raw-VFS access with the shared operator seam.

### Real TypeScript regression

Keep the stricter global-denial form of the Item 42 compiler test in
`packages/typescript/tests/typescript-tools.integration.test.ts` and require the
`finally` disposal to resolve. This verifies the full client/protocol/native
stack while leaving all behavior in the sidecar.

## Validation commands

Run at minimum:

```text
cargo test -p agentos-kernel --test permissions
cargo test -p agentos-native-sidecar --test filesystem
cargo test -p agentos-native-sidecar-browser --test wire_dispatch
pnpm --dir packages/typescript test -- typescript-tools.integration.test.ts
cargo check --workspace
cargo fmt --all -- --check
```

Rebuild `target/debug/agentos-sidecar` before the real TypeScript test so it
does not exercise a stale binary.

## Risks and dependencies

- **Security boundary:** the operator API must not be wired to guest filesystem
  RPC or executor syscalls. Existing guest `unmount_filesystem()` tests must
  remain unchanged and passing.
- **Do not mask real cleanup faults:** only bypass authorization. Busy mounts,
  invalid paths, plugin failures, audit failures, lifecycle failures, snapshot
  flush failures, and permission-map cleanup failures should still propagate as
  typed/aggregated cleanup errors.
- **Ordering:** keep process TERM/KILL handling before mount teardown. Avoid a
  temporary allow-all policy because incomplete process termination could let a
  surviving executor exploit that window.
- **Browser parity:** browser disposal is not the source of this reproduction,
  but its direct raw-VFS rollback is the same trust distinction expressed
  ad hoc. Converging it on the named kernel seam prevents the two adapters from
  drifting.
- **Related but separate:** the TypeScript duplicate wrapper messages expose a
  retry after the native VM has already been reclaimed. Fixing the authorization
  bug makes ordinary disposal succeed; changing committed-failure/retry
  semantics is lifecycle work and should not be folded into this item unless a
  remaining failure reproduces after the trusted unmount fix.

