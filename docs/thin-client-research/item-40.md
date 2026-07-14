# Item 40 research — make actor cron cold boot mandatory in CI

Status: implementation-ready research only, refreshed against `485b17d6` on
2026-07-14. This note does not modify production code, tests, workflows, or the
Item 40 tracker status.

Priority: **P1**. Confidence: **high**.

## Recommendation

Make `actor_cold_boot_restores_sidecar_owned_cron_state` fail when its real
sidecar prerequisite is absent, prove that the first shared sidecar was actually
disposed before the second boot, and run that test with an explicitly built
`agentos-sidecar` binary in regular CI, nightly CI, and `scripts/ci.sh`.

The current false green is directly reproducible, the positive path passes
against the real wrapper binary, and no production runtime or protocol change
is needed. The fix is test/CI enforcement plus assertions over public client
lifecycle state.

Do not mark this test `#[ignore]` and do not add another opt-out variable. Either
would preserve the same failure mode under a different spelling: the workflow
could report success without exercising teardown or restoration.

## Current code map

| Concern | Current symbol / anchor | Current behavior |
| --- | --- | --- |
| False-success prerequisite | `persistence_e2e::actor_cold_boot_restores_sidecar_owned_cron_state`, `crates/agentos-actor-plugin/src/persistence_e2e.rs:529-538` | Missing or non-file `AGENTOS_SIDECAR_BIN` prints a skip and returns `()`; Cargo reports success. |
| First boot, persistence, reboot | Same test, `persistence_e2e.rs:545-589` | Boots, saves opaque cron state, calls best-effort shutdown, boots again, verifies the job, then shuts down. It does not prove the first shutdown succeeded. |
| Actor VM bring-up/import | `crate::vm::ensure_vm`, `crates/agentos-actor-plugin/src/vm.rs:63-133` | Creates the real client VM and imports stored cron state before publishing the handle. |
| Actor shutdown | `crate::vm::shutdown_vm`, `vm.rs:160-175` | Takes the handle first, then logs and swallows `AgentOs::shutdown` failure. `vm.is_none()` alone is not teardown evidence. |
| Authoritative client shutdown | `AgentOs::shutdown`, `crates/client/src/agent_os.rs:457-539` | Closes the wire session, releases the lease, kills the last shared connection, disposes the sidecar, and removes it from the shared pool. |
| Test-visible lifecycle | `AgentOs::sidecar`, `agent_os.rs:575-579`; `AgentOsSidecar::describe`, `crates/client/src/sidecar.rs:215-228` | Public APIs expose handle identity, lifecycle state, and active VM count without test-only hooks. |
| Shared-process teardown | `AgentOsSidecar::kill_connection`, `sidecar.rs:199-206`; `dispose`, `sidecar.rs:239-278`; `AgentOsSidecarVmLease::dispose`, `sidecar.rs:293-310` | Last-lease shutdown kills the child, transitions to disposed, and evicts the exact shared-pool handle. |
| Regular CI omission | `.github/workflows/ci.yml:115-124` | Runs selected Rust crates and then deletes `target`; it neither builds the stable wrapper path nor runs actor tests. |
| Nightly omission | `.github/workflows/ci-nightly.yml:34-40` | Workspace tests run before the only sidecar build, and the later build is the wrong `agentos-native-sidecar` benchmark binary. |
| Local CI omission | `scripts/ci.sh:40-46` | Rust package tests stop at `agentos-client`; actor persistence is not invoked. |

## Original issue and observed evidence

The tracker entry is at `docs/thin-client-migration.md:86,166,251`. It requires
the real actor cron teardown/reboot path rather than a successful skip.

The only real cold-boot test is
`crates/agentos-actor-plugin/src/persistence_e2e.rs:529-596`. Its first two
branches currently print a message and return successfully when
`AGENTOS_SIDECAR_BIN` is unset or names a missing file. Running the exact test
without that environment variable produced this false green:

```console
$ env -u AGENTOS_SIDECAR_BIN cargo test -p agentos-actor-plugin \
    persistence_e2e::actor_cold_boot_restores_sidecar_owned_cron_state \
    -- --exact --nocapture
running 1 test
skipping actor cold-wake cron test: AGENTOS_SIDECAR_BIN is not set
test persistence_e2e::actor_cold_boot_restores_sidecar_owned_cron_state ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out
```

The same test was also run against the existing real wrapper binary:

```console
$ AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
    cargo test -p agentos-actor-plugin \
    persistence_e2e::actor_cold_boot_restores_sidecar_owned_cron_state \
    -- --exact --nocapture
test persistence_e2e::actor_cold_boot_restores_sidecar_owned_cron_state ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out
```

The current positive run started two real sidecar processes and completed the
test body in about 0.06 seconds after compilation. It emitted noisy
near-capacity warnings for the
`sidecar_stdout_frames` queue, but did not lose functionality. Treat that as a
separate observability concern, not a reason to weaken this test.

## What the current test really exercises

This is more than an in-memory persistence unit test:

1. `ensure_vm` in `crates/agentos-actor-plugin/src/vm.rs:63-133` starts an
   `AgentOs` VM through the configured `agentos-sidecar` wrapper and installs the
   actor SQLite-backed JS bridge.
2. The test schedules `survives-sleep` one hour in the future in the sidecar-owned
   cron registry, exports the opaque registry, and stores it in the mock actor's
   real in-memory SQLite database.
3. `shutdown_vm` takes the first `AgentOs` handle and calls
   `AgentOs::shutdown`. Because this test uses a UUID-unique shared pool with one
   VM, the client closes the VM's wire session, releases the last lease, kills
   the sidecar connection, disposes the pooled sidecar handle, and removes it
   from the pool (`crates/client/src/agent_os.rs:457-539` and
   `crates/client/src/sidecar.rs:199-206,239-278`).
4. The second `ensure_vm` therefore creates a new pooled sidecar handle/process,
   creates a new VM, loads the opaque SQLite value, and imports it into the new
   sidecar scheduler. Finding `survives-sleep` proves restoration across a real
   cold boot; the scheduled command never executes and needs no network.

There is one important test hole: `shutdown_vm` logs and swallows a failed
`AgentOs::shutdown` at `crates/agentos-actor-plugin/src/vm.rs:161-175`. If that
shutdown failed, the test could create another VM against the still-live pooled
sidecar and find the original in-memory cron job instead of proving restoration.
The implementation should close that hole in test assertions without changing
the actor's best-effort production shutdown contract.

The module header at `persistence_e2e.rs:1-8` also says “No VM, no sidecar.” That
is true for the storage-bridge tests but false for this cold-boot test and should
be narrowed as part of the test edit.

## Exact test edits

Edit `crates/agentos-actor-plugin/src/persistence_e2e.rs` only; no actor runtime
production edit is necessary.

### 1. Make the prerequisite fail closed

Replace the two skip-and-return branches at lines 531-539 with mandatory setup:

```rust
let sidecar_path = std::env::var("AGENTOS_SIDECAR_BIN")
    .expect("actor cold-boot cron E2E requires AGENTOS_SIDECAR_BIN");
let sidecar_file = std::path::Path::new(&sidecar_path);
assert!(
    sidecar_file.is_file(),
    "AGENTOS_SIDECAR_BIN does not point to a file: {}",
    sidecar_file.display()
);
```

Do not add a Unix executable-bit assertion to the Rust test. `is_file` gives a
clear configuration diagnostic on every supported host, the CI shell can check
`-x` on Linux, and `ensure_vm` already propagates an actual spawn failure.

### 2. Prove teardown and a distinct cold boot

Immediately before the first `shutdown_vm`, retain the public sidecar handle.
`AgentOs::sidecar` is public at `crates/client/src/agent_os.rs:575-579`, and
`describe` exposes lifecycle state and lease count at
`crates/client/src/sidecar.rs:215-228`:

```rust
let first_sidecar = first.sidecar();
crate::vm::shutdown_vm(&host, &mut vm, "sleep").await;
assert!(vm.is_none(), "sleep must drop the first VM handle");
let first_description = first_sidecar.describe();
assert_eq!(first_description.active_vm_count, 0);
assert_eq!(
    first_description.state,
    agentos_client::SidecarState::Disposed,
    "sleep must dispose the one-VM sidecar before cold boot"
);
```

After the second `ensure_vm`, retain its sidecar and prove that the pool did not
reuse the disposed handle:

```rust
let restored = vm.as_ref().expect("restored VM");
let restored_sidecar = restored.sidecar();
assert!(
    !std::sync::Arc::ptr_eq(&first_sidecar, &restored_sidecar),
    "cold boot must allocate a new sidecar handle"
);
assert_eq!(restored_sidecar.describe().active_vm_count, 1);
```

Keep the existing restored-job assertion: that is the actual import proof. After
the final `shutdown_vm`, assert that `vm.is_none()` and that
`restored_sidecar.describe()` is `SidecarState::Disposed` with zero active VMs
as a cleanup assertion.

```rust
crate::vm::shutdown_vm(&host, &mut vm, "destroy").await;
assert!(vm.is_none(), "destroy must drop the restored VM handle");
let restored_description = restored_sidecar.describe();
assert_eq!(restored_description.active_vm_count, 0);
assert_eq!(
    restored_description.state,
    agentos_client::SidecarState::Disposed,
    "destroy must dispose the restored one-VM sidecar"
);
```

This is stronger than checking only `vm.is_none()`: `shutdown_vm` calls
`vm.take()` before attempting the fallible shutdown, so `None` by itself cannot
prove the sidecar/session was closed.

### 3. Correct the module description

Revise `persistence_e2e.rs:1-8` to say that most tests isolate the real SQLite
storage bridge without a VM, while `actor_cold_boot_restores_sidecar_owned_cron_state`
intentionally launches a real sidecar and VM to prove opaque cron restoration.

Do not change `crates/agentos-actor-plugin/src/vm.rs`, client lifecycle code,
protocol schemas, Cargo manifests, or lockfiles for Item 40.

## Exact CI edits

The required executable is the wrapper package/bin `agentos-sidecar`, declared
in `crates/agentos-sidecar/Cargo.toml`; it is **not**
`agentos-native-sidecar`. The wrapper is the same binary resolved by normal
Rust-client startup and includes the native runtime implementation.

### `.github/workflows/ci.yml`

The `rust` job currently downloads/copies the WASM command artifact and runs
clippy plus selected crate tests, but it never runs actor-plugin tests or sets
`AGENTOS_SIDECAR_BIN`. Add these steps after the current clippy step and before
the existing `Reclaim Cargo target space before package tests` step:

```yaml
      - name: Build sidecar for actor persistence E2E
        run: cargo build -p agentos-sidecar
        env:
          CARGO_TARGET_DIR: ${{ github.workspace }}/target
      - name: Test actor cron cold boot
        run: |
          test -x "$AGENTOS_SIDECAR_BIN"
          cargo test -p agentos-actor-plugin persistence_e2e -- --test-threads=1 --nocapture
        env:
          CARGO_TARGET_DIR: ${{ github.workspace }}/target
          AGENTOS_SIDECAR_BIN: ${{ github.workspace }}/target/debug/agentos-sidecar
```

Use the module filter `persistence_e2e`, matching the tracker checklist, rather
than running only the named test. It keeps the cheap SQLite behavior tests next
to the real cold-boot assertion. Explicit `cargo build` is required: `cargo
test` may compile a hashed test executable but does not promise the stable
`target/debug/agentos-sidecar` binary path.

The actor package is already a root workspace member (`Cargo.toml:3-24`), but
the regular `rust` job currently runs only protocol, sidecar, and client tests
at `.github/workflows/ci.yml:115-120`.

The build/test must remain after the WASM commands are copied and before `cargo
clean`. Do not inherit `AGENT_OS_CLIENT_ALLOW_E2E_SKIPS=1` from the client-test
step; this actor step must have no skip permission.

### `.github/workflows/ci-nightly.yml`

Nightly currently executes `cargo test --workspace` at
`.github/workflows/ci-nightly.yml:34` before its release native-sidecar build.
Add a debug wrapper build immediately before the workspace tests and pass the
stable path to that test step:

```yaml
      - run: cargo build -p agentos-sidecar
        env:
          CARGO_TARGET_DIR: ${{ github.workspace }}/target
      - run: cargo test --workspace -- --test-threads=1
        env:
          CARGO_TARGET_DIR: ${{ github.workspace }}/target
          AGENTOS_SIDECAR_BIN: ${{ github.workspace }}/target/debug/agentos-sidecar
```

Keep the later release build of `agentos-native-sidecar`; it serves the benchmark
matrix and cannot satisfy this earlier actor test or its wrapper-binary contract.

### `scripts/ci.sh`

The repository's local CI entry point likewise omits the actor plugin: its Rust
list ends with `agentos-client` at `scripts/ci.sh:40-45`. Define a target
directory that respects an existing caller override, then build and run the
actor persistence module after the existing Rust package tests and before
`pnpm check-types`:

```bash
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"
export CARGO_TARGET_DIR

run_step cargo build -p agentos-sidecar
run_step test -x "${CARGO_TARGET_DIR}/debug/agentos-sidecar"
run_step env \
  AGENTOS_SIDECAR_BIN="${CARGO_TARGET_DIR}/debug/agentos-sidecar" \
  cargo test -p agentos-actor-plugin persistence_e2e -- --test-threads=1
```

Do not hard-code `${ROOT_DIR}/target` while ignoring an existing
`CARGO_TARGET_DIR`; developers and CI runners may place Cargo output elsewhere.

## Before/after validation checklist

### Before behavior

- [ ] **Before test:** record the current false-success command and output:
  `env -u AGENTOS_SIDECAR_BIN cargo test -p agentos-actor-plugin
  persistence_e2e::actor_cold_boot_restores_sidecar_owned_cron_state -- --exact
  --nocapture`.
- [ ] Confirm it prints `skipping actor cold-wake cron test` and reports one
  passing test. This is the authoritative “before” evidence, not desirable
  behavior to preserve. It reproduced on `485b17d6` with eight filtered unit
  tests.

### After behavior

- [ ] **After prerequisite test:** run that same command without
  `AGENTOS_SIDECAR_BIN`; it must fail at the explicit prerequisite rather than
  pass.
- [ ] Run `cargo build -p agentos-sidecar` after the WASM command assets are
  available.
- [ ] **After cold-boot test:** run
  `AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" cargo test -p
  agentos-actor-plugin persistence_e2e -- --test-threads=1 --nocapture`.
- [ ] Confirm the cold-boot test proves the first sidecar is disposed with zero
  leases, the second sidecar handle is distinct and active, the job is restored,
  and final cleanup disposes the second sidecar with zero leases.
- [ ] Parse both changed workflow YAML files and run `bash -n scripts/ci.sh`.
- [ ] Run the cheap repository gates appropriate to the touched Rust test and CI
  files: `cargo fmt --check` and `cargo clippy -p agentos-actor-plugin --tests --
  -D warnings`.
- [ ] Mark all three Item 40 tracker checkboxes and the work-item row complete
  only after the mandatory CI command passes.

## Risks and non-goals

- The test is Linux CI-compatible and uses a unique pool, but the pool and the
  process-global sidecar cache are shared state. Keep `--test-threads=1` for the
  actor persistence module/workspace test.
- The scheduled cron command is one hour in the future and is cancelled after
  restore. The test needs no external network and should not wait for execution.
- SQLite is already a bundled actor-plugin dev dependency; no system SQLite or
  Cargo dependency change is needed.
- A panic between boot and shutdown can leave a child until the test process
  exits. The isolated CI process and transport kill-on-drop behavior make a new
  production teardown guard unnecessary for this item.
- `Arc::ptr_eq` proves that the process-global pool returned a distinct sidecar
  handle; the first handle's `Disposed` state plus the client shutdown path's
  `kill_connection` is what establishes that this was a process cold boot, not
  merely a second VM on the same handle.
- Do not fold the noisy sidecar stdout-frame warnings, actor shutdown error
  policy, or client-wide E2E skip policy into Item 40. They are separate changes.

## Dedicated `jj` revision

Create Item 40 as one dedicated revision on top of completed Item 39, for example:

```text
test(actor): require cron cold-boot persistence
```

Expected revision scope:

- `crates/agentos-actor-plugin/src/persistence_e2e.rs`
- `.github/workflows/ci.yml`
- `.github/workflows/ci-nightly.yml`
- `scripts/ci.sh`
- `docs/thin-client-migration.md`
- `docs/thin-client-research/item-40.md`

No production Rust source, generated protocol, Cargo manifest, or lockfile should
move in this revision.
