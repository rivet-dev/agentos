# Item 78 research: make the kernel root use the sidecar Linux bootstrap contract

Status: implementation-ready research only. This note does not modify
production code, tests, the tracker, or JJ state.

Inspected on **2026-07-14** on Item 42 (`suwmustu`). The real gate was run
against the rebuilt `target/debug/agentos-sidecar`, not a mock or the old
in-process TypeScript kernel.

Priority: **P1**. Root-cause confidence: **high (99%)**. Recommended-fix
confidence: **high (95%)**.

## Recommendation

Keep the test expectations and fix the runtime. The authoritative kernel root
and the sidecar's shadow/native-root helpers currently use two different
bootstrap implementations. Give the kernel root a lowest-priority standard
root layer, with path-specific modes, and make the native sidecar reuse the same
bounded directory table.

The important layering order is:

```text
explicit bootstrap upper
    -> caller-provided lowers
    -> bundled base image (unless disabled)
    -> standard AgentOS/Linux root fallback
```

The fallback must be lower than the bundled base. That makes it fill missing
mountpoints such as `/etc/agentos` and `/usr/local/bin` without changing real
base-image metadata such as `/tmp` `01777`, `/root` `0700`, or the ownership of
`/workspace`.

When the bundled base is disabled and the caller supplied no lower, that same
fallback is the minimal root. It must assign `/tmp` and `/var/tmp` mode `01777`
explicitly; it must not call the generic `FilesystemEntry::directory` helper
and accept its ordinary `0755` default for those paths.

Do not add bootstrap entries, default modes, or post-create `mkdir`/`chmod`
calls to either client. Root construction happens before VM readiness through
trusted runtime code and therefore does not require guest filesystem rights.

## Original issue and exact root cause

The current real result is:

| Case | Expected | Actual |
| --- | --- | --- |
| Bundled base, `/tmp` | `01777` | `01777` (passes) |
| Bundled base, gap-fill directories | `/etc/agentos` and `/usr/local/bin` exist | `/etc/agentos` is absent; the loop fails on its first entry |
| `disableDefaultBaseLayer: true`, no lower | `/tmp` is `01777` | `/tmp` is `0755` |

The request path is:

```text
TypeScript AgentOs.create
  -> CreateVmConfig (root config omitted, or only disableDefaultBaseLayer=true)
  -> agentos-sidecar wrapper
  -> agentos_native_sidecar::vm::create_vm
  -> agentos_native_sidecar_core::build_root_mount_table_with_loaded_snapshot
  -> vfs::posix::RootFileSystem::from_descriptor_with_import_limits
```

The failure is in the last function, in
`crates/vfs/src/posix/root_fs.rs`:

```rust
if !descriptor.disable_default_base_layer {
    lower_snapshots.push(load_bundled_base_snapshot_with_limits(limits)?);
} else if lower_snapshots.is_empty() {
    lower_snapshots.push(minimal_root_snapshot());
}
```

### Default base: no gap-fill layer is constructed

With the base enabled, only the bundled JSON snapshot is appended. The private
`DEFAULT_ROOT_DIRECTORIES` table is never read. The committed VFS base asset
contains `/tmp`, `/usr/local`, and `/workspace`, but it contains neither
`/etc/agentos` nor `/usr/local/bin`:

```text
crates/vfs/assets/base-filesystem.json
  /tmp          mode 1777
  /usr/local    mode 0755
  /workspace    mode 0755 uid=1000 gid=1000
  (no /etc/agentos)
  (no /usr/local/bin)
```

That makes the first test pass from base-image metadata and the second fail.
Nothing overwrites or deletes `/etc/agentos`; it is never installed in the
authoritative root.

### No base: the minimal table throws away path-specific modes

`minimal_root_snapshot` maps every string in `DEFAULT_ROOT_DIRECTORIES` through:

```rust
FilesystemEntry::directory(path)
```

That public constructor deliberately defaults an ordinary directory to `0755`.
It has no reason to recognize `/tmp`, so both `/tmp` and `/var/tmp` lose their
sticky bit. Changing `FilesystemEntry::directory` itself would be wrong because
it is also the generic default for explicit caller-created directories.

The minimal table also lacks `/home/agentos` and `/workspace`, even though the
native shadow-root table and the default VM cwd contract include them. That is
an untested instance of the same split and should be corrected while the table
is made authoritative.

### `SHADOW_ROOT_BOOTSTRAP_DIRS` is real, but not the kernel root

`crates/native-sidecar/src/vm.rs` declares the modes the failing tests expected:

```rust
("/tmp", 0o1777),
// ...
("/usr/local/bin", 0o755),
("/var/tmp", 0o1777),
("/etc/agentos", 0o755),
("/workspace", 0o755),
```

That table is used only by:

- `bootstrap_shadow_root`, which prepares the host shadow tree used by mapped
  native execution; and
- `bootstrap_native_root_filesystem`, which prepares an explicitly configured
  native root plugin.

Normal overlay VMs use `RootFileSystem` from the VFS crate. `vm.stat()` and
`vm.exists()` therefore report the VFS root, not the shadow tree. The table is
not dead, but the existing comments and Item 2 explanation incorrectly imply
that it also seeds the normal kernel root.

## Are the tests or production stale?

**Production is wrong for the two asserted behaviors.** `/tmp` `01777` is a
standard Linux/container invariant and is required for normal multi-process
temporary-file behavior. `/usr/local/bin` is already on the sidecar-owned
default `PATH`, so providing the directory is internally consistent.

`/etc/agentos` is product-specific rather than a generic Linux directory, but
the runtime still reserves and protects that subtree in
`crates/kernel/src/kernel.rs` and native shadow synchronization. Until that
reserved subtree is deliberately removed as a separate product decision, the
sidecar-owned root contract should provide its mountpoint consistently. An
empty directory is not a reason to restore client bootstrap.

The prose at the top of
`packages/core/tests/kernel-bootstrap-base.test.ts` **is stale**. The bundled
base does not provide every POSIX/AgentOS directory, and there is no generic
"host-side bootstrap emits nothing" rule. Rewrite the comment to describe a
low-priority fallback layer that fills gaps without clobbering higher-layer
metadata. The assertions themselves should remain and be broadened.

## Exact production edits

### 1. `crates/vfs/src/posix/root_fs.rs`

Replace the string-only private `DEFAULT_ROOT_DIRECTORIES` with one bounded,
exported path/mode table. A suitable name is
`AGENTOS_ROOT_BOOTSTRAP_DIRECTORIES`; it should exclude `/` so native-root and
host-shadow loops can consume it safely.

Retain the existing bounded path set, and include the two currently shadow-only
working directories:

```rust
pub const AGENTOS_ROOT_BOOTSTRAP_DIRECTORIES: &[(&str, u32)] = &[
    ("/dev", 0o755),
    ("/proc", 0o755),
    ("/tmp", 0o1777),
    // existing standard paths, each with its intentional mode
    ("/home/agentos", 0o755),
    ("/usr/local/bin", 0o755),
    ("/var/tmp", 0o1777),
    ("/etc/agentos", 0o755),
    ("/workspace", 0o755),
];
```

Do not broaden Item 78 into an unreviewed base-image metadata rewrite. The
bundled base already has additional intentional metadata (`/root` `0700`,
`/sys` and `/var/empty` `0555`, `/home/agentos` `02755`, and user ownership for
`/home/agentos` and `/workspace`). Higher-layer precedence must preserve it.
The fallback table should initially preserve the existing sidecar table's
no-base modes except for the already-declared sticky directories.

Build `minimal_root_snapshot` from an explicit `/` entry plus that table, set
each entry's mode from the tuple, then retain the existing empty `/usr/bin/env`
fallback. Do not special-case generic `FilesystemEntry::directory`.

Change lower selection to append the standard fallback below the base:

```rust
let mut lower_snapshots = descriptor.lowers.clone();
if !descriptor.disable_default_base_layer {
    lower_snapshots.push(load_bundled_base_snapshot_with_limits(limits)?);
    lower_snapshots.push(minimal_root_snapshot());
} else if lower_snapshots.is_empty() {
    lower_snapshots.push(minimal_root_snapshot());
}
```

This intentionally preserves the current meaning of an explicit custom root:
when the default base is disabled and at least one caller lower is supplied,
the caller owns that lower's shape. It also avoids silently increasing the
minimum inode count of custom roots and breaking the existing small import-limit
tests. The ordinary no-base/no-lower VM still gets a viable Linux root.

The fallback is trusted and bounded, but it is a real lower layer. Keep it in
`validate_descriptor_import_limits` so its inode/memory cost remains visible to
resource limits.

### 2. `crates/native-sidecar/src/vm.rs`

Import `AGENTOS_ROOT_BOOTSTRAP_DIRECTORIES` through
`agentos_kernel::root_fs` and delete the duplicate
`SHADOW_ROOT_BOOTSTRAP_DIRS` constant. Use the shared table in both
`bootstrap_shadow_root` and `bootstrap_native_root_filesystem`.

This is the only native-sidecar change required. Do not copy the fallback into
`create_vm`, do not generate protocol bootstrap entries, and do not run these
operations after guest permissions are active.

### 3. Confirmed adjacent dead base staging

`crates/native-sidecar/build.rs` copies a second
`assets/base-filesystem.json` into `OUT_DIR`, but no native-sidecar Rust source
includes or reads that staged file. The authoritative parser is
`crates/vfs/src/posix/root_fs.rs`, which embeds the VFS crate's asset. The two
committed JSON assets are byte-identical today.

After the root fix is green, remove the unused native-sidecar build script and
duplicate asset, and remove the corresponding native-sidecar copy from the
"Stage vendored V8 bridge bundles and base filesystem" publish step. Keep the
VFS asset and the VFS build script: the latter also stages the package-format
schema and is not dead. This cleanup is high-confidence but separable if the
main Item 78 diff must stay minimal.

## Exact tests

### Before behavior already recorded

Keep the current real-client gate as the before test:

```bash
cargo build -p agentos-sidecar --bin agentos-sidecar
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/core exec vitest run \
  tests/kernel-bootstrap-base.test.ts --fileParallelism=false
```

Expected parent result: **1 passed, 2 failed**. The base `/tmp` test passes;
the gap-fill test fails at `/etc/agentos`; the no-base mode comparison receives
`0755` instead of `01777`.

### Sidecar/runtime tests after the fix

1. In `crates/vfs/tests/posix_root_fs.rs`, add
   `default_root_fallback_fills_base_gaps_without_clobbering_base_metadata`.
   Create the default descriptor and assert:
   - `/etc/agentos` and `/usr/local/bin` exist at `0755`;
   - `/tmp` and `/var/tmp` remain `01777` from the higher base;
   - `/root` remains `0700`; and
   - `/workspace` remains uid/gid `1000`, proving the fallback did not copy up
     or replace base metadata.
2. In the same file, add
   `minimal_root_uses_standard_directory_modes_without_the_base`. Disable the
   base with no lowers and assert `/tmp` and `/var/tmp` are `01777`, while
   `/etc/agentos`, `/usr/local/bin`, `/home/agentos`, and `/workspace` exist.
3. Add or extend a custom-lower test to prove
   `disable_default_base_layer: true` plus an explicit lower does not acquire
   the fallback and that explicit bootstrap directory metadata still wins.
   This freezes the scoped semantics above and protects the import-limit tests.
4. Extend `bootstrap_shadow_root_seeds_standard_directories` in
   `crates/native-sidecar/src/vm.rs` to iterate the shared table and check every
   path exists. Retain the explicit `/tmp` `01777` assertion.
5. Extend the native-root plugin test in the same module to stat `/tmp` after
   `bootstrap_native_root_filesystem` and assert `01777`; this proves the
   second consumer of the shared table.
6. Extend
   `create_vm_bootstrap_needs_no_guest_filesystem_rights` in
   `crates/native-sidecar/tests/service.rs`. Under `PermissionsPolicy::deny_all`,
   assert the kernel root contains `/etc/agentos`, `/usr/local/bin`, and
   `/workspace`, and that `/tmp` is `01777`; retain the post-readiness `EACCES`
   write assertion. This is the authoritative proof that bootstrap does not
   require guest rights.
7. Add a small browser-sidecar service regression using a no-base/no-lower
   root and its root snapshot. Assert the `/tmp` snapshot entry has mode
   `01777` and the same gap directories exist. Browser and native share the VFS
   root constructor, but this locks the adapter path to that implementation.

### Real client black-box gate after the fix

Update `packages/core/tests/kernel-bootstrap-base.test.ts` rather than adding
client implementation logic:

- replace the stale header comment;
- check the same directory list and `/tmp`/`/var/tmp` modes in a table-driven
  helper for both default and no-base/no-lower VMs; and
- keep this as a black-box protocol gate only. All detailed behavior tests live
  in the sidecar/VFS suites above.

No Rust client-specific implementation test is needed for the defaults. The
Rust client already preserves omission and sends only explicit root config;
the shared sidecar tests are the parity authority. A single Rust black-box
smoke may be added if desired, but it must not duplicate the mode table in
client code.

## Validation commands

```bash
cargo fmt --all -- --check
cargo test -p agentos-vfs-core --test posix_root_fs
cargo test -p agentos-native-sidecar --lib bootstrap_shadow_root_seeds_standard_directories
cargo test -p agentos-native-sidecar --test service create_vm_bootstrap_needs_no_guest_filesystem_rights -- --nocapture
cargo test -p agentos-native-sidecar-browser --test service -- root_bootstrap
cargo check -p agentos-native-sidecar -p agentos-native-sidecar-browser
cargo build -p agentos-sidecar --bin agentos-sidecar
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/core exec vitest run \
  tests/kernel-bootstrap-base.test.ts --fileParallelism=false
```

If the dead staging cleanup is included, also run the repository publish helper
checks that validate crate contents and the workflow script. The cleanup must
not remove `crates/vfs/assets/base-filesystem.json`.

## Completion criteria

- Default and no-base/no-lower roots expose the documented bounded directory
  contract and sticky modes through the authoritative kernel VFS.
- Higher base/custom metadata is not clobbered by the fallback.
- Native shadow roots, native root plugins, native overlay VMs, and browser
  overlay VMs consume the same runtime-owned path/mode table where applicable.
- A deny-all guest policy does not block trusted root construction and is
  restored before readiness.
- TypeScript and Rust clients continue to omit defaults and perform no startup
  filesystem mutation.
- The real `agentos-sidecar` black-box gate passes all cases.
