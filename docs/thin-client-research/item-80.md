# Item 80 research: remove implicit native host-path execution

## Recommendation

Upgrade Item 80 from **P2 to P0** (high confidence) and remove the whole
compatibility path in one revision. The original regression only demonstrates
an absolute execute `cwd` beneath `vm.host_cwd` being translated and created in
the guest. The same design also lets an untrusted JavaScript guest name a raw
host JavaScript file in `child_process`: `prepare_javascript_shadow` can read
that host path directly, copy it into the VM shadow, and execute it without a
`host_dir` mount. This crosses the declared sidecar-to-executor security
boundary, not merely an SDK-thinness boundary.

The target rule should be simple:

- every `cwd`, command path, and file entrypoint arriving from Execute or a
  guest child-process request is a **guest Linux path**;
- relative paths resolve against the applicable guest cwd;
- an absent guest path returns its Linux errno;
- a host path is reachable only through an explicit `host_dir` mount, after
  which the request still names the mount's guest path;
- host paths used internally to launch V8, Pyodide, or Wasmtime are derived
  from an already-authorized guest path, never from the original specifier.

## Why the current behavior is unsafe

The top-level cwd flow in `crates/native-sidecar/src/execution.rs` is:

```text
Execute.cwd
  -> resolve_execution_cwds
  -> if absolute and beneath vm.host_cwd:
       strip the host prefix, invent a guest path, retain the raw host cwd
  -> validate_or_materialize_execution_cwd
  -> if the guest path is absent but the host directory exists:
       mkdir the guest path
```

Entrypoints have a parallel path:

```text
absolute entrypoint
  -> resolve_host_entrypoint_within_vm_host_cwd
  -> translate host path to guest path
  -> prepare_javascript_shadow
  -> materialize_host_path_to_shadow
```

There is an even broader fallback in `prepare_javascript_shadow`: if the guest
entrypoint is absent, any absolute `resolved.entrypoint` that exists on the
host is passed to `materialize_host_path_to_shadow`. The helper calls raw
`symlink_metadata`, `read_link`, or `fs::read` and then writes the result into
the VM shadow. It does not require a mount and is reachable from
`resolve_javascript_child_process_execution`, so hostile guest code can request
`node /some/host/secret.mjs` or spawn an absolute `.js` command. The existing
`execute_rejects_host_only_absolute_command_path` test covers only a top-level
shell-like command outside the VM host cwd; it does not exercise this
JavaScript path.

## Exact production edits

All paths below are in `crates/native-sidecar/src/execution.rs` unless stated
otherwise.

### 1. Make top-level cwd resolution guest-only

Replace `resolve_execution_cwds`' three-value result with a two-value result:

```rust
fn resolve_execution_cwds(
    vm: &VmState,
    value: Option<&str>,
) -> Result<(String, PathBuf), SidecarError> {
    let guest_cwd = resolve_guest_execution_cwd(vm, value)?;
    let host_cwd = resolve_vm_guest_path_to_host(vm, &guest_cwd);
    Ok((guest_cwd, host_cwd))
}
```

Delete all of the following:

- the `Path::is_absolute` / `path_is_within_root(vm.host_cwd)` branch;
- the `allow_host_path_overrides` boolean;
- the special `value.is_none() => vm.host_cwd.clone()` branch;
- `validate_or_materialize_execution_cwd`'s host-directory fallback.

Use `vm.kernel.validate_process_cwd(&guest_cwd).map_err(kernel_error)` directly
in `resolve_execute_request` and `resolve_command_execution`. Item 42 already
added this Linux `chdir` validation; Item 80 should preserve it while deleting
the compatibility `mkdir`.

This makes an absolute host-looking cwd ordinary guest input. For example,
`/tmp/agentos-fixture/work` means exactly that guest path. It succeeds only if
that path exists in the VFS (including through a mount), otherwise it returns
`ENOENT`.

### 2. Delete top-level entrypoint translation

In all three entrypoint branches in `resolve_execute_request` and
`resolve_command_execution`:

- direct `runtime + entrypoint`;
- `node <entrypoint>`;
- direct `.js` / `.mjs` / `.cjs` command;

delete `requested_host_entrypoint`, `host_entrypoint_override`, the
`allow_host_path_overrides` gates, and the `"outside sandbox root"` errors.
Delete `resolve_host_entrypoint_within_vm_host_cwd` after its remaining shadow
caller is removed.

Resolve the specifier exactly once with `guest_entrypoint_for_specifier` (or
`resolve_guest_path` for a required path), put that guest path in
`AGENTOS_GUEST_ENTRYPOINT`, and validate it through the kernel before process
admission. Do not place the caller's raw absolute string into an engine
`module_path` or `file_path`.

The clean implementation is one helper shared by top-level and child path
branches:

```rust
fn resolve_existing_guest_entrypoint(
    vm: &mut VmState,
    guest_cwd: &str,
    specifier: &str,
) -> Result<String, SidecarError> {
    let guest_path = resolve_path_like_guest_specifier(guest_cwd, specifier);
    vm.kernel.lstat(&guest_path).map_err(kernel_error)?;
    Ok(guest_path)
}
```

Code/eval modes (`node -e`, Python `-c`, Python `-m`, stdin, and interactive
mode) must remain exempt because their entrypoint field is source or a module
selector, not a filesystem path.

### 3. Derive engine host paths only from the VFS

Delete the raw-host branch in `prepare_javascript_shadow` and then delete its
now-dead helpers:

- `materialize_host_path_to_shadow`;
- `sync_shadow_entrypoint_into_kernel`.

`prepare_javascript_shadow` should only:

1. read the normalized guest entrypoint from `AGENTOS_GUEST_ENTRYPOINT`;
2. return immediately for an explicit `host_dir` mount;
3. otherwise call `materialize_guest_path_to_shadow`, which reads through the
   kernel VFS and therefore honors mounts, symlink rules, permissions, and
   Linux errno.

Also remove the `vm.host_cwd` allowance from
`load_javascript_entrypoint_source`. Kernel VFS reads are authoritative. If a
host-side fallback remains for a staged runtime file, restrict it to `vm.cwd`
(the sidecar-owned shadow root) only; never accept `vm.host_cwd` or the original
specifier as another trusted root.

WebAssembly needs an explicit staging adjustment because the engine consumes a
host module path. Before `CreateWasmContextRequest`, resolve/realpath the guest
entrypoint in the kernel, then:

- use `host_mount_path_for_guest_path` when it is backed by an explicit
  `host_dir` mount; or
- call `materialize_guest_path_to_shadow` and use
  `shadow_path_for_guest(vm, guest_path)`.

The existing AgentOS-package Wasm staging can be generalized or reused, but the
host path must always be derived from the validated guest path. Otherwise
removing only the JavaScript fallback would leave direct Wasm requests passing
raw host-looking strings to Wasmtime.

### 4. Make guest child cwd and entrypoint resolution identical

In `resolve_javascript_child_process_execution`:

- replace the cwd tuple/override state machine with
  `resolve_guest_path(parent_guest_cwd, cwd)`;
- derive `host_cwd` with
  `host_runtime_path_for_guest_path_with_env` (explicit mount/runtime mapping)
  or `shadow_path_for_guest`; inheritance may retain `parent_host_cwd` only
  when the guest cwd is unchanged;
- remove the absolute-path-within-`parent_host_cwd` translation;
- remove the relative `parent_host_cwd.join(cwd)` override;
- remove the `vm.host_cwd` fallback;
- replace both child spawn sites' `validate_or_materialize_execution_cwd(...,
  true)` with kernel-only cwd validation.

For path-like child commands and `node <entrypoint>`, resolve and validate the
guest entrypoint first. The host execution path may then come only from the
explicit runtime guest-path mapping or the VM shadow. Remove the fallback that
uses an absolute `guest_entrypoint` directly as a host `PathBuf`.

This child path is essential to Item 80's completion: top-level requests are
sent by the trusted client, but child-process requests originate from the
untrusted executor.

### 5. Remove legacy external-cwd host synchronization

The compatibility cwd becomes observable after execution because
`sync_active_process_host_writes_to_kernel` and
`sync_process_host_writes_to_kernel` recursively import a process host cwd when
it lies outside `vm.cwd`. Once every process host cwd is either the VM shadow or
an explicit mount, that whole external-root synchronization path is obsolete.

Delete:

- `collect_active_process_host_sync_roots`;
- `collect_process_host_sync_roots`;
- the out-of-shadow branch in `sync_process_host_writes_to_kernel`;
- the loop over `extra_roots` in `sync_active_process_host_writes_to_kernel`.

Keep ordinary shadow-root reconciliation if the runtime still requires it.
`host_dir` writes are already performed by `HostDirFilesystem`; recursively
copying the mount's host root back into the kernel is neither needed nor the
right enforcement path.

### 6. Adjacent VM-create ambiguity should be fixed with this item or tracked

`crates/native-sidecar/src/vm.rs::resolve_vm_cwds` still treats every absolute
`CreateVmConfig.cwd` as a host path, even though `agentos-vm-config` validates
and documents it as a guest path and the SDKs forward it as such. This is why
many native fixtures currently establish `vm.host_cwd` by putting a temp host
directory in metadata `cwd`.

The consistent fix is:

```rust
fn resolve_vm_cwds(
    configured_cwd: Option<&String>,
    shadow_root: &Path,
) -> Result<(String, PathBuf), SidecarError> {
    let guest_cwd = resolve_guest_cwd(configured_cwd);
    Ok((
        guest_cwd.clone(),
        shadow_path_for_guest(shadow_root, &guest_cwd),
    ))
}
```

Then delete `resolve_host_path` if it has no other caller. Host workspaces must
be mounted, not smuggled through VM `cwd`. If the main implementation keeps
this out of Item 80, create a separate tracked P1 item; otherwise the public VM
cwd contract remains host-dependent and migrated tests can accidentally retain
the old root through another entrypoint.

## Fixture and caller migration inventory

The largest source is
`crates/native-sidecar/tests/support/mod.rs`:

- `create_vm_wire_with_metadata` injects the host temp directory as metadata
  `cwd`;
- `execute_wire` serializes an `&Path` host entrypoint directly;
- `create_vm`, `create_vm_with_metadata`, and `execute` wrap those behaviors.

Change the execute helpers to accept a guest `&str`. Add a test helper that
builds an explicit mount such as:

```rust
MountDescriptor {
    guest_path: String::from("/workspace"),
    read_only: Some(false),
    plugin: MountPluginDescriptor {
        id: String::from("host_dir"),
        config: Some(json!({
            "hostPath": host_workspace,
            "readOnly": false,
        }).to_string()),
    },
}
```

Create the VM with omitted cwd (exercising the `/workspace` default) or an
explicit guest cwd, initialize/configure this mount, and execute
`/workspace/<fixture>`. Read-only probes should use a read-only mount unless the
test intentionally observes writes.

Every current caller of the host-serializing `execute_wire` helper must migrate;
the affected files are:

- `builtin_completeness.rs`, `builtin_conformance.rs`;
- `crash_isolation.rs`, `process_isolation.rs`, `session_close.rs`,
  `vm_lifecycle.rs`;
- `fetch_via_undici.rs`, `fs_watch_and_streams.rs`, `guest_identity.rs`;
- `kill_cleanup.rs`, `signal.rs`, `socket_state_queries.rs`,
  `posix_compliance.rs`;
- `posix_path_repro.rs`, `promisify_module_load.rs`, `security_audit.rs`,
  `security_hardening.rs`;
- `node_modules_host_mount_resolution.rs` and
  `node_modules_symlink_resolution.rs`.

`posix_path_repro.rs`, `security_audit.rs`,
`node_modules_host_mount_resolution.rs`, and
`node_modules_symlink_resolution.rs` already send nonempty mount patches. Their
mount lists must include the workspace mount instead of replacing it; do not
hide implicit remounting inside `execute_wire` because that would erase or
reorder the mount topology the test intends to exercise.

Additional direct host-entrypoint fixtures that do not solely rely on
`execute_wire` occur in:

- `security_hardening.rs` (direct JavaScript entrypoint and nested host cwd);
- `signal.rs` (direct entrypoint request);
- `python.rs` (custom Python execute helpers); and
- `service.rs` (direct Python/Wasm entrypoint requests and Item 42's
  `host-nested` cwd regression).

Custom VM-create helpers that inject a host directory into metadata `cwd` occur
in `builtin_conformance.rs`, `guest_identity.rs`, `fs_watch_and_streams.rs`,
`python.rs`, and `security_hardening.rs`, in addition to the shared support
helper. Non-execution fixtures using the same legacy pattern also exist in
`connection_auth.rs`, `session_isolation.rs`, `kill_cleanup.rs`,
`layer_management.rs`, and `stdio_binary.rs`; they should use an omitted/guest
cwd even if they do not need a workspace mount.

Expected assertion updates include `guest_identity.rs` and security probes that
currently expect `process.cwd()` / `PWD` to be `/` only because a host metadata
cwd is translated to the guest root. Mounted workspace executions should expect
`/workspace` (or their explicit guest cwd).

## Before/after test checklist

### Characterize before deletion

- [ ] Keep Item 42's current regression: create a directory only below
  `vm.host_cwd`, send the absolute host path as Execute `cwd`, and prove the old
  code starts the process and manufactures a guest directory.
- [ ] Add a direct-entrypoint escape regression in `security_hardening.rs`:
  create a valid host-only `.mjs` outside every mount, send its absolute path as
  a JavaScript entrypoint, and prove the old sidecar reads/executes it.
- [ ] Add an untrusted child regression: execute a guest script from an explicit
  `/workspace` mount, have it run `node <absolute-host-only.mjs>`, and prove the
  old child path exposes a unique host-only sentinel.
- [ ] Add the equivalent direct Wasm characterization if the engine accepts the
  raw host module path; this guards the non-JavaScript engine path.

### Validate after deletion

- [ ] Convert the Item 42 host-cwd regression: the same absolute string is an
  ordinary guest path and returns `kernel_error` with `ENOENT`; assert no guest
  directory and no active/retained process were created.
- [ ] Direct host-only JavaScript, Python-file, and Wasm entrypoints return
  Linux errno before process admission, never emit the sentinel, and leave no
  engine context/process.
- [ ] A guest child `node <host-only-path>` returns `ENOENT`, never executes or
  copies the file, and leaves no child route.
- [ ] A relative and absolute guest cwd both succeed when present; a missing
  cwd returns `ENOENT`, a file cwd returns `ENOTDIR`, and no case creates it.
- [ ] The same fixture succeeds at `/workspace/entry.mjs` with an explicit
  `host_dir` mount. Assert argv/PWD/cwd are guest paths while intentional host
  writes appear only under the mounted directory.
- [ ] Read-only `host_dir` entrypoints load successfully and writes return
  `EROFS`/`EACCES` through the VFS rather than falling back to a shadow copy.
- [ ] Nested JavaScript `child_process`, Python subprocess, and Wasm command
  paths retain guest cwd and explicit-mount behavior.
- [ ] Run the complete native sidecar service, security-hardening, builtin,
  POSIX, Python, signal, process/session lifecycle, filesystem, and mount suites.
- [ ] Add or retain one native/browser conformance case showing that a
  host-looking absolute string has identical guest-path semantics in both
  adapters.

## Risks and implementation order

1. Add the escape regressions first. They prevent a partial fix that deletes
   cwd translation but leaves raw-host entrypoint materialization.
2. Add the explicit workspace-mount test helper and migrate fixtures. This
   separates intended host fixture access from the compatibility code before
   production deletion.
3. Delete top-level and child cwd translation/materialization.
4. Delete raw-host entrypoint translation/materialization and add VFS-derived
   Wasm staging.
5. Remove external-cwd host sync and the VM-create host-cwd interpretation.
6. Run the broad native suite; fixture failures that mention missing modules or
   entrypoints usually indicate a missing explicit mount or a caller still
   sending a host path, not a reason to restore compatibility.

The main compatibility risk is engine implementation: V8/Pyodide/Wasmtime need
host paths internally. Preserve those host paths as private derived state, but
derive them only from `host_dir` mappings or sidecar-owned shadow files after a
successful kernel VFS lookup. The public protocol and guest child APIs must
never carry them.

## Final assessment

| Priority | Fix confidence | Scope confidence | Reason |
|---|---:|---:|---|
| **P0** | **High** | **High** | Raw host entrypoint materialization is reachable from untrusted guest child-process input and bypasses the explicit-mount boundary. The deletion points and replacement path already exist: kernel guest-path resolution, `host_dir` mappings, and guest-to-shadow staging. |
