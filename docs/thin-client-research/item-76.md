# Item 76 research — own Rust sidecar transport runtime lifetime

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 76 tracker status.

Inspected: **2026-07-14**, shared working copy `6a9155972ee8`. Symbol names and
code shapes below are the stable anchors if earlier stacked items move line
numbers.

## Priority, confidence, and recommendation

- Priority: **P1**.
- Root-cause confidence: **high**. The default-parallel cron suite failed in two
  of three runs, both tests passed separately and with `--test-threads=1`, and
  the task ownership follows directly from `tokio::spawn` inside
  `SidecarTransport::spawn`.
- Fix confidence: **high**. Run all native sidecar transport I/O on one bounded,
  lazily initialized Tokio runtime owned for the Rust process lifetime. Spawn
  the child and its reader, writer, watchdog, and sidecar-request tasks on that
  runtime, then return the same `Arc<SidecarTransport>` API to callers.

Do not make VM/session/cron policy client-owned and do not move the fix into the
sidecar protocol. Framed stdio and native child-process ownership are legitimate
host transport responsibilities. The sidecar remains authoritative for VMs,
cron state, permissions, filesystems, and every runtime decision.

Do not weaken the public shared-pool contract to a thread-local or
runtime-local pool. `AgentOsSidecar` is intentionally process-global and should
continue to share one process/connection across callers. The transport driver
must have a lifetime at least as long as that process-global connection.

## Original issue and exact ownership chain

### A process-global object captures runtime-local tasks

`common::new_vm()` in `crates/client/tests/common/mod.rs:65-66` uses a default
`AgentOsConfig`. `AgentOs::create` resolves that omission to the process-global
`"default"` shared sidecar at `crates/client/src/agent_os.rs:202-210`.

The shared state is explicit:

- `SHARED_SIDECARS` is a process-global map at
  `crates/client/src/sidecar.rs:313-320`;
- `AgentOsSidecar.connection` at `sidecar.rs:120-122` retains one
  `SharedConnection` for the pool;
- `SharedConnection` at `sidecar.rs:27-32` retains the shared
  `Arc<SidecarProcess>` and authenticated connection ID; and
- `AgentOsSidecar::ensure_connection` at `sidecar.rs:144-197` creates that
  transport only for the first caller, then gives every later VM a clone.

The transport's lifetime is not actually process-global. At
`crates/sidecar-client/src/transport.rs:573-620`,
`SidecarTransport::spawn` creates the child and calls:

```rust
tokio::spawn(run_writer(stdin, control_writer_rx, request_writer_rx));
tokio::spawn(run_reader(Arc::downgrade(&transport), stdout));
tokio::spawn(run_silence_watchdog(
    Arc::downgrade(&transport),
    SIDECAR_SILENCE_TIMEOUT,
));
```

Those tasks belong to the Tokio runtime that happened to call `spawn` first.
The transport stores channels and the child, but no runtime-independent driver.
Dropping that first runtime aborts all three tasks even while another runtime
still holds a live VM and the process-global pool still holds the transport.

`dispatch_sidecar_request` at `transport.rs:950-982` also uses ambient
`tokio::spawn`. It must run on the same owned transport runtime so a host
callback cannot inherit a short-lived request caller's runtime either.

### Lease accounting correctly keeps the process, but cannot keep the runtime

Every VM increments the shared `active_vm_count` in
`AgentOs::create` at `crates/client/src/agent_os.rs:355-359`. Shutdown releases
only that VM's lease. At `agent_os.rs:527-535`, the client kills the connection
and disposes the sidecar only when the count reaches zero.

That is correct same-process sibling ownership. It creates the observed broken
state when runtime A exits while VM B remains:

```text
process-global AgentOsSidecar (active_vm_count = 1)
  -> cached SharedConnection
    -> cached Arc<SidecarTransport>
      -> child still retained
      -> request/control senders still retained
      -> writer/reader/watchdog tasks aborted with runtime A
```

VM B's next request reaches `request_wire_with_frame_limit` at
`transport.rs:704-751`. With the writer receiver gone, the send at lines
728-733 returns `sidecar transport closed`. If a response was already enqueued
when the reader disappeared, the waiter instead receives
`sidecar transport disconnected`; the reader's normal tail cleanup at lines
1192-1194 does not run when the task itself is aborted.

### Cron exposed the bug but does not own it

The Item 37 tests made the failure visible because each `#[tokio::test]` owns a
separate runtime and both originally called `common::new_vm()` against the same
default pool. Whichever test created the transport first owned its I/O tasks.
If that test completed first, its runtime disappeared while the sibling test's
one-second alarm was firing.

Cron itself is correctly isolated:

- each `AgentOs` constructs its own `CronManager` at
  `crates/client/src/agent_os.rs:361-376`;
- alarm state, alarm handler, and alarm task are per manager at
  `crates/client/src/cron.rs:218-233` and `392-469`;
- cron requests contain connection/session/VM ownership at
  `cron.rs:892-898`; and
- the native sidecar validates that ownership and stores schedulers by `vm_id`
  at `crates/native-sidecar/src/service.rs:1619-1663` and `1688-1712`.

The two tests use distinct job IDs. Their host callback IDs both begin at
`host-cron-callback-1`, but those registries are per `CronManager` and their
runs are returned through VM-owned requests. Neither IDs nor timestamp equality
caused the failure. The near-identical timestamps merely make runtime A's exit
race VM B's alarm frequently.

Item 37 now uses distinct test pools in
`crates/client/tests/cron_e2e.rs:21-22` and `155-156`. Keep that isolation: it
removes cross-test coupling. It is not the product fix for Item 76.

## Exact recommended production edit

### `crates/sidecar-client/Cargo.toml`

Add Tokio's `rt-multi-thread` feature to this crate's existing dependency. Do
not add a new async runtime dependency:

```toml
tokio = { version = "1", features = [
  "io-util", "macros", "process", "rt", "rt-multi-thread", "sync", "time"
] }
```

The workspace already resolves Tokio with this feature through other crates, so
no version or protocol change is needed. Regenerate `Cargo.lock` only if Cargo
actually changes it.

### `crates/sidecar-client/src/transport.rs`

1. Import `std::sync::OnceLock` alongside `Arc`/`Weak`.

2. Add one bounded process-lifetime transport runtime near the transport
   constants. Use a fixed worker count (recommended: two) and descriptive
   thread names:

   ```rust
   const TRANSPORT_RUNTIME_WORKERS: usize = 2;

   static TRANSPORT_RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> =
       OnceLock::new();

   fn transport_runtime() -> Result<&'static tokio::runtime::Runtime, TransportError> {
       TRANSPORT_RUNTIME
           .get_or_init(|| {
               tokio::runtime::Builder::new_multi_thread()
                   .worker_threads(TRANSPORT_RUNTIME_WORKERS)
                   .thread_name("agentos-sidecar-transport")
                   .enable_all()
                   .build()
                   .map_err(|error| error.to_string())
           })
           .as_ref()
           .map_err(|error| {
               TransportError::Sidecar(format!(
                   "failed to initialize sidecar transport runtime: {error}"
               ))
           })
   }
   ```

   Storing the `Runtime`, rather than only an ambient `Handle`, makes ownership
   explicit and prevents a caller runtime from determining task lifetime. Two
   workers keep the resource bound fixed while ensuring a reader and writer can
   progress when a sidecar-request callback is awaiting work. Do not use
   `spawn_blocking` or an unbounded thread-per-request design.

3. Split the current `SidecarTransport::spawn` body into a private
   `spawn_on_transport_runtime` async function. It should contain the existing
   binary resolution, `Command`, pipes, channels, object construction, and the
   three background spawns unchanged.

4. Make the public `SidecarTransport::spawn` submit that whole function to the
   owned runtime and await the join result:

   ```rust
   pub async fn spawn(binary_path: Option<String>) -> Result<Arc<Self>, TransportError> {
       transport_runtime()?
           .spawn(Self::spawn_on_transport_runtime(binary_path))
           .await
           .map_err(|error| {
               TransportError::Sidecar(format!(
                   "sidecar transport startup task failed: {error}"
               ))
           })?
   }
   ```

   Because `spawn_on_transport_runtime` executes inside the owned runtime, its
   existing `tokio::spawn` calls bind to that runtime. The child must also be
   created there: creating Tokio process pipes on runtime A and moving them to a
   different reactor is not a valid fix.

5. Leave `request_wire*`, event logs, pending bounds, callback maps, ownership
   routing, and payload codecs unchanged. Callers continue awaiting ordinary
   transport futures on their own runtimes; Tokio MPSC/oneshot/watch channels
   are runtime-independent synchronization boundaries.

6. Keep `kill_child`, `AgentOsSidecar::kill_connection`, and last-lease disposal
   semantics. When the final transport `Arc` disappears, its senders and
   `kill_on_drop` child close; the owned runtime lets reader/writer cleanup run
   instead of aborting it with an unrelated caller runtime.

No edit belongs in `crates/native-sidecar`, the wire schema, cron, TypeScript,
or Rust VM policy. This is a generic native framed-transport lifetime fix.

## Deterministic before regression

Add `crates/client/tests/shared_sidecar_runtime_e2e.rs` as a separate integration
test binary. Do not use timing or default test parallelism to reproduce the
failure.

The test should be a synchronous `#[test]` that creates two explicit Tokio
runtimes on different OS threads and coordinates them with standard channels:

1. Choose one unique shared pool name used only by this test.
2. On OS thread/runtime A, create VM A first and signal readiness. This
   deterministically makes runtime A create the pooled `SidecarTransport`.
3. On runtime B, create VM B against the same pool. Assert the sidecar IDs are
   equal and `active_vm_count == 2`.
4. Signal runtime A to shut down VM A and exit its thread/runtime. Join that
   thread before continuing. Assert VM B's sidecar count is now one.
5. Under a short outer timeout, issue a simple real request from VM B, such as
   `list_cron_jobs()` or a filesystem round trip.
6. Schedule a near-future VM B host callback, wait under one bounded deadline
   for the callback and its terminal event, then cancel it.
7. Shut down VM B normally.

Against the parent implementation, step 5 deterministically fails with a
transport-closed/disconnected error after runtime A drops. Record that exact
test name, parent revision, and failure in the tracker before applying the fix.
The test must not accept a timeout as success, and its own waits must be bounded
so a missing reader cannot hang CI.

This test proves more than the original cron race:

```text
runtime A creates shared process -> runtime B leases same process
runtime A VM closes -> runtime A exits -> runtime B still performs RPC + alarm/wake
```

## After tests

The same deterministic test must pass without serialization or distinct pools
between A and B. Also retain:

- `crates/client/tests/sidecar_pool_e2e.rs`, which proves two VMs on one runtime
  share a process, remain isolated, and release independently;
- `crates/client/tests/cron_e2e.rs`, under default parallel test threads, which
  proves success/failure callback completion without cross-test pool coupling;
- all `agentos-sidecar-client` unit tests, especially request disconnect,
  pending cleanup, event routing, writer priority, and silence watchdog cases;
  and
- a teardown assertion in the new regression that VM B can shut down after
  runtime A is gone, so the last lease still kills the child and removes the
  cached pool entry.

Do not replace the deterministic regression with `--test-threads=1`. Serial
execution only hides the process-global/runtime-local mismatch.

## Validation commands

```sh
cargo build -p agentos-sidecar

cargo test -p agentos-sidecar-client --lib -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test shared_sidecar_runtime_e2e -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test sidecar_pool_e2e -- --nocapture
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  cargo test -p agentos-client --test cron_e2e -- --nocapture

cargo check -p agentos-sidecar-client -p agentos-client
cargo fmt --all -- --check
git diff --check
```

Run the new cross-runtime test repeatedly during implementation; one pass is
the deterministic acceptance, while repetition catches driver startup/teardown
races:

```sh
for attempt in 1 2 3 4 5; do
  AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
    cargo test -p agentos-client --test shared_sidecar_runtime_e2e -- --nocapture || exit 1
done
```

Finish with the repository cheap gates:

```sh
cargo check --workspace
pnpm build
pnpm check-types
```

## Risks, boundaries, and dependencies

- **Spawn the child on the owned runtime.** Moving only `run_reader`/writer
  after creating Tokio pipes on the caller reactor is incomplete and may fail
  when that reactor disappears.
- **Sidecar-request callbacks:** `dispatch_sidecar_request` inherits the
  transport runtime through ambient `tokio::spawn`. Its callback type is
  `Send + Sync + 'static`, so it may run there. Keep at least two bounded workers
  so one awaiting callback cannot stop framed input/output progress. Callbacks
  must remain asynchronous; blocking user work is not transport policy.
- **No per-request runtime/thread:** one process-owned bounded runtime drives all
  transport tasks. Do not create an OS thread for every request or callback.
- **Failure visibility:** runtime initialization and startup task failure must
  return `TransportError`; never fall back to the caller runtime or silently
  recreate a second sidecar.
- **Teardown:** the static runtime itself lives until process exit, but each
  child, channel, event subscription, and pending request must retain its
  existing transport/lease lifetime. The fix must not keep a strong transport
  reference in a permanent driver task.
- **Cross-runtime synchronization:** do not hold a caller-runtime lock or Tokio
  mutex guard across the submitted startup task. `AgentOsSidecar::connection`
  already serializes first connection establishment.
- **Resource bounds:** keep the runtime worker count a fixed constant and retain
  every existing queue, event-log, pending-request, and silence bound.
- **Item 37:** its distinct cron test pools are a valid test-isolation change and
  should remain. Item 76 owns the generic cross-runtime product guarantee.
- **No TypeScript parity edit:** Node has one process event loop, so it cannot
  exhibit this Rust multi-runtime lifetime mismatch. Public shared-sidecar
  behavior remains identical.

## Dedicated Item 76 `jj` revision

Create one stacked revision after prior tracker items are sealed. Suggested
description:

```text
fix(rust): own shared sidecar transport runtime
```

Expected bounded path set:

- `crates/sidecar-client/Cargo.toml`
- `crates/sidecar-client/src/transport.rs`
- `crates/client/tests/shared_sidecar_runtime_e2e.rs`
- `docs/thin-client-migration.md` (Item 76 evidence/status only)
- `Cargo.lock` only if Cargo actually changes it

No client cron implementation, TypeScript, native sidecar, runtime-core,
protocol schema, website, registry, package lock, or public API file should
change. Record the deterministic parent failure, passing after test, teardown
coverage, validation commands, and dedicated revision ID before marking Item 76
done.
