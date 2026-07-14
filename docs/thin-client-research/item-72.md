# Item 72 research: compact Rust terminal process routes

Status: implementation-ready research only. This note does not modify
production code, tests, or the Item 72 tracker status.

## Recommendation

Split the Rust process registry entry into explicit `Running` and `Terminal`
variants. A running entry should own the wire process ID, stdout/stderr
broadcast senders, exit watch sender, and initial output-callback task handles.
On success or route failure, atomically replace that entry with only the typed
`ProcessExit` result and its completion sequence, then prune compact terminal
entries to the sidecar-advertised `process_route_retention`.

Priority: **P2**. Confidence: **high**.

The intended ownership is:

```text
running Rust host route
  = process_id + stdout/stderr senders + exit watch + callback task handles
                         |
                         | ordered ProcessExited / route failure
                         v
compact Rust terminal route
  = typed ProcessExit + sidecar-bounded completion sequence
```

Do not add a sidecar request, protocol frame, client default, timer, or second
retention policy. The sidecar already resolves and advertises the retention
count. The remaining client state is host-only correlation for Rust APIs that
the sidecar cannot invoke: an already-created `wait_process` receiver, a late
exit callback, or a late stdout/stderr stream.

## Original issue

The numbered summary, status row, and checklist are currently at
`docs/thin-client-migration.md:118,200,287`:

> Rust now bounds terminal entries using the sidecar-advertised count, but each
> retained entry still owns broadcast senders and output callback task handles.
> Compact terminal success/failure entries as TypeScript does while preserving
> typed late wait/subscription parity.

`ProcessEntry` is currently one struct for both states. Completion only changes
`terminal_sequence` from zero to a positive number. Therefore every retained
terminal entry continues to own:

- stdout and stderr `broadcast::Sender` values;
- an exit `watch::Sender`;
- the sidecar wire `process_id` string; and
- every `JoinHandle` created for `SpawnOptions.on_stdout` and `on_stderr`.

Those retained broadcast senders keep the callback receivers open. The task
handles are finally aborted only by VM shutdown, even if the process completed
long before shutdown. With the default sidecar-advertised route count of 1,024,
the bounded registry can consequently retain thousands of channel allocations,
process IDs, and task handles where only an exit code or typed route failure is
needed.

## Cross-layer inventory

| Layer | Exact location | Item 72 disposition |
| --- | --- | --- |
| Rust host registry | `crates/client/src/agent_os.rs:50-79,127-137,331-371,590-604` | Change the entry representation and shutdown matching. Keep consuming the advertised bound without a client default. |
| Rust process surface | `crates/client/src/process.rs:296-527,666-695,780-887,1021-1046,1140-1241` | Install a running entry, compact it with the exact terminal result, prune terminal variants, and make late APIs match the compact state. |
| Rust byte stream | `crates/client/src/stream.rs:102-117` | Add an already-ended stream constructor for late successful output subscriptions. |
| TypeScript host registry | `packages/core/src/agent-os.ts:1273-1294,1669-1704,1742-1813,2111-2131` | No edit. It already replaces running routes with compact completed/failed records and defines the parity behavior Rust should preserve. |
| Sidecar default | `crates/native-sidecar-core/src/limits.rs:17-28` | No edit. The authoritative default is 1,024 and an explicit higher `maxProcesses` raises it. |
| Wire schema | `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:569-576,597-605` | No edit. `VmCreatedResponse` and `VmInitializedResponse` already carry `processRouteRetention`. |
| Generated TypeScript codec | `packages/runtime-core/src/generated-protocol.ts:3115-3147` | No edit or regeneration. It already serializes the field. |
| Rust protocol compatibility | `crates/sidecar-protocol/src/protocol.rs:841-843,1009-1011,1589` | No edit. Existing aliases/pass-through preserve the generated field. |
| Native/browser advertisement | `crates/native-sidecar/src/vm.rs:335-396`; `crates/native-sidecar-browser/src/wire_dispatch.rs:1639-1659,1763-1777` | No edit. Both adapters resolve and advertise the sidecar-owned count. |
| Rust tests | `crates/client/src/process.rs:1306-1577`; `crates/client/src/agent_os.rs:2081-2173`; `crates/client/src/stream.rs`; `crates/client/tests/process_e2e.rs:171-238` | Add/adapt the focused lifecycle, late-route, shutdown, stream, and E2E assertions described below. |

There is no generated Rust source to edit for this item. The BARE schema and
generated TypeScript codec already express the required contract; Item 72 is a
Rust host-resource lifecycle correction behind that unchanged contract.

## Exact current code and behavior

### One full entry represents both states

`crates/client/src/agent_os.rs:50-79` defines `ProcessExit` and the current
`ProcessEntry`. `terminal_sequence: AtomicU64` uses zero as the live-state tag;
the remaining fields exist in both the live and terminal state.

`AgentOsInner` at `agent_os.rs:127-137` stores:

- `process_registry_lock`, which serializes completion/pruning;
- `process_route_retention`, decoded directly from the initialize response;
- `next_process_terminal_sequence`; and
- `processes: SccHashMap<u32, ProcessEntry>`.

The advertised count is read at `agent_os.rs:331-346` and installed without a
client default at `agent_os.rs:363-371`. Preserve that behavior.

### Spawn installs channels and callback tasks

`crates/client/src/process.rs:296-386` creates stdout/stderr broadcast channels
and an exit watch channel. `install_output_callback` is called for each initial
callback at lines 321-327, and its `JoinHandle` is stored on the full entry at
lines 348-355.

`install_output_callback` at `process.rs:1184-1226` subscribes a task to a
broadcast receiver. The task exits only on a typed route failure or after every
sender is dropped. Its current documentation explicitly notes that the entry's
sender clones prevent normal channel closure.

### Completion marks the full entry instead of replacing it

`run_spawn_events` at `process.rs:813-887` forwards ordered output and terminal
events:

- a transport lag sends `Lagged` to both output routes and
  `ProcessExit::EventStreamLagged` to the exit watch, then fail-closes the wire
  process;
- transport closure sends typed `Closed` output events and
  `ProcessExit::EventStreamClosed`; and
- `ProcessExitedEvent` sends `ProcessExit::Exited(exit_code)`.

After the loop, `record_process_terminal_and_prune` at lines 1170-1182 merely
stores a sequence number in the existing full entry. `prune_terminal_processes`
at lines 1157-1168 scans every entry and interprets a nonzero atomic value as
terminal. Entries within the retained window still own all live-route fields.

### Late APIs depend on terminal correlation, not live senders

The following methods currently read fields from the full struct:

- `on_process_stdout` / `on_process_stderr` at `process.rs:429-451` subscribe
  to retained senders, then inspect the exit watch;
- `on_process_exit` at lines 453-505 subscribes to the watch and invokes a late
  success callback synchronously;
- `wait_process` at lines 507-527 returns an already-stored terminal result or
  waits on a live watch receiver; and
- `lookup_process_id` at lines 689-695 supplies the wire ID to stdin and signal
  operations.

`byte_stream_for_process_route` at lines 1033-1046 already synthesizes a typed
late output error for lag/closure. For a successful exit it returns a receiver
from the retained sender, even though no later data can arrive.

`write_process_stdin`, `close_process_stdin`, and `signal_process` at current
lines 389-427 and 780-804 always send a wire request because the terminal entry
still has `process_id`. Their comments promise a successful terminal route is a
no-op. Compaction should make the implementation match those comments and the
TypeScript behavior.

### Shutdown is the only current full cleanup

`close_process_routes` at `agent_os.rs:590-604` sends terminal failures through
every entry's channels and then calls `drain_process_output_tasks`.

`drain_process_output_tasks` at `process.rs:1228-1241` removes all entries,
takes all callback handles, and aborts them. This remains appropriate for
still-running routes during authoritative VM shutdown, but it should not be the
normal lifetime of callbacks belonging to a process that already exited.

### TypeScript already has the desired shape

`packages/core/src/agent-os.ts:1273-1294` has separate
`RunningProcessRoute`, `CompletedProcessRoute`, and `FailedProcessRoute` types.
`_trackProcess` at current lines 1669-1704 clears handler sets and replaces the
running entry with `{ state: "exited", exitCode }` or
`{ state: "failed", error }` before pruning.

Its public terminal behavior at `agent-os.ts:1742-1813,2111-2131` is:

- stdin writes, stdin close, stop, and kill are no-ops after successful exit;
- late stdout/stderr subscriptions contain no live handler route;
- a late exit callback fires immediately;
- a late wait resolves immediately; and
- a stored route failure is rethrown.

Rust should achieve the same semantics with Rust's stream/watch API, not by
retaining live channels.

## Exact production edits

### `crates/client/src/agent_os.rs`

Replace the single full `ProcessEntry` struct with an enum and a running-only
payload. Suggested shape:

```rust
pub(crate) enum ProcessEntry {
    Running(RunningProcessRoute),
    Terminal {
        exit: ProcessExit,
        terminal_sequence: u64,
    },
}

pub(crate) struct RunningProcessRoute {
    pub stdout_tx: broadcast::Sender<RoutedStreamEvent<Vec<u8>>>,
    pub stderr_tx: broadcast::Sender<RoutedStreamEvent<Vec<u8>>>,
    pub exit_tx: watch::Sender<Option<ProcessExit>>,
    pub process_id: String,
    pub output_tasks: Vec<JoinHandle<()>>,
}
```

The terminal variant must not contain sender values, a process ID, task handles,
or an atomic live/terminal sentinel. Keep `next_process_terminal_sequence` on
`AgentOsInner`; the registry lock still serializes assignment and pruning, so a
plain `u64` is sufficient inside a terminal entry. `AtomicU64` remains needed
for the global sequence allocator.

Update `close_process_routes` to send `EventStreamClosed` only through
`ProcessEntry::Running` entries. Terminal entries already carry their final
result and have no live channels. Then drain the complete map as today.

### `crates/client/src/process.rs`: registration and transition

Wrap the entry created by `spawn` in `ProcessEntry::Running`. Keep all live
fields and callback installation behavior unchanged.

Replace `record_process_terminal_and_prune` with a helper such as
`compact_process_terminal_and_prune` that receives the PID, exact
`ProcessExit`, completion sequence, and advertised retention count. The caller
continues to hold `process_registry_lock`; use `SccHashMap::update` to replace a
running entry:

```rust
*entry = ProcessEntry::Terminal {
    exit,
    terminal_sequence,
};
```

Do not abort `output_tasks` during normal compaction. Assignment drops each
`JoinHandle`, which detaches rather than aborting its task, and drops the
registry's sender clones. Once `run_spawn_events` returns and drops its own
sender clones, callback receivers can drain already-queued output in protocol
order, observe channel closure, and end naturally. Aborting at the transition
can discard output that was sent before `ProcessExitedEvent` but has not yet
been scheduled in the callback task.

Change `run_spawn_events` so its event loop returns the exact `ProcessExit` it
sent to `exit_tx`, then pass that value into the compaction helper. The smallest
mechanical form is:

```rust
let terminal_exit = loop {
    let event = match events.recv().await {
        Ok(event) => event,
        Err(WireEventRecvError::Lagged { skipped }) => {
            let exit = ProcessExit::EventStreamLagged { skipped };
            let _ = stdout_tx.send(RoutedStreamEvent::Lagged { skipped });
            let _ = stderr_tx.send(RoutedStreamEvent::Lagged { skipped });
            let _ = exit_tx.send(Some(exit.clone()));
            self.abort_wire_process_after_route_failure(&process_id, "spawn")
                .await;
            break exit;
        }
        Err(WireEventRecvError::Closed) => {
            let exit = ProcessExit::EventStreamClosed;
            let _ = stdout_tx.send(RoutedStreamEvent::Closed {
                context: "process output route closed before process exit",
            });
            let _ = stderr_tx.send(RoutedStreamEvent::Closed {
                context: "process output route closed before process exit",
            });
            let _ = exit_tx.send(Some(exit.clone()));
            break exit;
        }
    };
    if event.ownership != ownership {
        continue;
    }
    match &event.payload {
        EventPayload::ProcessExitedEvent(event) if event.process_id == process_id => {
            let exit = ProcessExit::Exited(event.exit_code);
            let _ = exit_tx.send(Some(exit.clone()));
            break exit;
        }
        // existing ordered stdout/stderr forwarding and unrelated-event cases
    }
};

let _registry_guard = self.inner().process_registry_lock.lock();
let terminal_sequence = self
    .inner()
    .next_process_terminal_sequence
    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
compact_process_terminal_and_prune(
    &self.inner().processes,
    pid,
    terminal_exit,
    terminal_sequence,
    self.inner().process_route_retention,
);
```

Retain the current complete stdout/stderr and unrelated-event arms in the
elided match. Preserve the current ordering:

1. send output route failure where applicable;
2. send the terminal value to the exit watch so existing waiters wake promptly;
3. perform the existing route-failure wire abort where applicable; and
4. atomically compact and prune.

Do not infer the terminal value later from the watch sender. Carrying one typed
value through the pump avoids a second state machine and guarantees the compact
entry matches the result observed by in-flight receivers.

Update `prune_terminal_processes` to collect sequences only from
`ProcessEntry::Terminal`. Update `terminal_pids_to_prune` to accept only actual
terminal `(pid, sequence)` pairs; zero no longer means live. Preserve
completion-order pruning and the exact sidecar-advertised count, including zero.

Update `drain_process_output_tasks` to remove every registry entry but extract
and abort handles only from `ProcessEntry::Running`. A terminal variant should
be removed silently because it cannot contain tasks. Keeping the existing
helper name is fine; its shutdown-only role remains accurate.

### `crates/client/src/process.rs`: late routes and controls

Match on `ProcessEntry` in every registry reader:

- `on_process_stdout` and `on_process_stderr`: subscribe only when running; for
  a terminal success return an already-ended `ByteStream`, and for terminal lag
  or closure return the existing typed failed stream;
- `on_process_exit`: subscribe to the running watch, or process the terminal
  value immediately with the same synchronous success/logged-failure behavior;
- `wait_process`: subscribe when running, or call `process_exit_result` directly
  when terminal;
- `lookup_process_id`: replace it with a helper returning
  `Result<Option<String>, ClientError>`; return `Some(id)` when running,
  `None` for `Exited`, and the typed `process_exit_result` failure for a terminal
  route failure; and
- `write_process_stdin`, `close_process_stdin`, and `signal_process`: return
  `Ok(())` without a wire request for `None`.

Do not hold an `scc` read guard while invoking a caller callback. Copy the
terminal value or create the running receiver inside `read`, release the guard,
and only then call the handler or await.

`list_processes` and `get_process` need only key membership and therefore do not
need semantic changes. Do not make snapshot absence mean terminal expiry; Item
71 owns that separate protocol contract.

### `crates/client/src/stream.rs`

Add a crate-private constructor for an already-ended byte stream, for example:

```rust
pub(crate) fn empty() -> Self {
    let (tx, rx) = broadcast::channel(1);
    drop(tx);
    Self::new(rx)
}
```

This allocates only an ephemeral channel for that returned stream. It does not
retain senders or a route in the registry. Keep `ByteStream::failed` unchanged
for a one-shot typed lag/closed error.

Refactor `byte_stream_for_process_route` into explicit running and terminal
paths (or an equivalent small enum-returning helper). Avoid creating a dummy
receiver before the registry lookup for terminal routes.

### No sidecar, protocol, or TypeScript production change

The sidecar already owns the retention policy in
`crates/native-sidecar-core/src/limits.rs:17-28` and advertises the resolved
value in VM initialization. Rust already consumes that field. Moving callback
senders or Rust `FnOnce` handlers into the sidecar is neither possible nor
desirable; they are host-language resources.

Do not modify:

- native or browser sidecar process retention;
- initialize response frames;
- TypeScript route types;
- process snapshot lookup/expiry semantics; or
- VM limit defaults.

## Before and after tests

### Before and after: one durable callback-lifetime regression

Write the intended final regression first, run it against the vulnerable
parent, retain the failure output for the tracker, and keep the same test after
the production change. Suggested name:
`terminal_transition_releases_output_callbacks_after_draining_ordered_output`.
It should:

1. create stdout/stderr and exit channels, install one callback, and retain its
   `AbortHandle` for observation;
2. insert the current full entry for the red run, then
   `ProcessEntry::Running` for the final version;
3. enqueue a final output chunk before the terminal result and retain an
   in-flight exit watch receiver;
4. send the exit value, invoke the current mark helper for the red run (the
   compact helper afterward), and drop the pump's external sender
   clones;
5. assert the callback receives the queued final chunk;
6. assert its `AbortHandle` eventually becomes finished without calling
   `abort`, proving sender removal closed the receiver naturally.
7. in the final version, also assert the map matches only
   `ProcessEntry::Terminal { exit, terminal_sequence }`.

For the red-before run, make the smallest test-only adaptation needed to build
against the old full `ProcessEntry` and invoke
`record_process_terminal_and_prune`. The callback-finished assertion must fail:
retaining the terminal entry retains the sender, so the callback task cannot
finish.
After introducing the enum and compaction helper, adapt only the construction
and helper call; the same intended assertion turns green. Do not record a
temporary test that merely confirms the bug and then delete it—the lasting
regression is what prevents sender/task retention from returning.

Use the same focused command for both evidence runs:

```bash
cargo test -p agentos-client \
  process::tests::terminal_transition_releases_output_callbacks_after_draining_ordered_output \
  -- --exact
```

Record the vulnerable parent revision plus the expected timeout/not-finished
assertion in the tracker before implementing, then record the green command on
the dedicated Item 72 revision.

The enum shape makes sender/task absence a compile-time property. Do not add
dummy `Option<Sender>` or empty task vectors to the terminal variant merely to
make a runtime field assertion possible.

The existing tests at `process.rs:1390-1499` are supporting evidence: they prove
the registry owns callback handles and shutdown aborts them, but they do not
currently exercise ordinary process completion.

### After: compaction, pruning, and in-flight receivers

Adapt `terminal_pruning_preserves_an_in_flight_wait_receiver` at current
`process.rs:1527-1577` to the enum. It must still prove that:

- the oldest compact terminal route is removed by completion order;
- the newest compact terminal route remains;
- live routes are never included in terminal pruning; and
- a watch receiver acquired while running observes the exact terminal result
  even after the map entry is replaced and later pruned.

Add a zero-retention case. The compact route may be removed immediately, but a
receiver obtained before terminal delivery must still resolve because the watch
value is sent before replacement/removal.

### After: late success/failure parity

Extend `crates/client/tests/process_e2e.rs` immediately after the existing
successful `cat` wait at current lines 230-238. While the route is within the
advertised retention window, assert:

- a second `wait_process(pid)` immediately returns the same exit code;
- a late `on_process_exit` invokes its callback synchronously exactly once;
- late stdout and stderr streams terminate without yielding data;
- `write_process_stdin`, `close_process_stdin`, `stop_process`, and
  `kill_process` all return `Ok(())`; and
- the no-op controls issue no semantic error after success.

There is an intentional scheduling edge to account for: `run_spawn_events`
sends the exit watch value before it acquires `process_registry_lock` and
replaces the registry entry. Therefore the first `wait_process` can wake just
before compaction. Immediately construct the late stdout/stderr streams and
await their termination under a short timeout before asserting the second wait,
late exit callback, and no-op controls. Stream closure both synchronizes with
sender removal and makes an accidentally retained/open sender fail
deterministically instead of hanging the suite.

Add a focused unit test for the new control target resolver as well. It should
prove `Running` returns `Some(process_id)`, terminal success returns `None`
(therefore no request can be assembled), terminal lag/closure returns the exact
typed error, and an absent PID remains `ProcessNotFound`. The E2E establishes
the public `Ok(())` behavior; this unit test establishes that successful late
controls really skip the wire rather than relying on a permissive sidecar.

Retain and adapt these current typed failure tests in `process.rs`:

- `late_process_output_subscriber_receives_retained_route_failure` at current
  lines 1377-1388;
- `closed_process_event_stream_is_an_error_not_exit_zero` at lines 1506-1511;
  and
- the lagged `process_exit_result` coverage near lines 1306-1318.

Add a direct compact-terminal failure case for both `EventStreamLagged` and
`EventStreamClosed`: late wait/control resolution must return the same typed
`ClientError`, and late stdout/stderr must emit one typed error then terminate.
Do not convert failures into exit code zero, empty success, or
`ProcessNotFound`.

Add one `stream.rs` unit test that `ByteStream::empty().next().await` returns
`None` immediately. Keep the existing failed-stream test green so the empty
success constructor cannot accidentally erase a typed route failure.

### Shutdown coverage

Update `confirmed_shutdown_terminally_clears_all_host_route_collections` in
`crates/client/src/agent_os.rs:2081-2173` to insert a running enum variant. Keep
its assertions that authoritative shutdown sends route closure, empties the
registry, and aborts callback tasks for processes that are still live.

Add a compact terminal entry to the same test or a smaller adjacent test and
assert shutdown simply removes it. It has no sender/task to notify or abort.

## Research baseline

On research revision `22adc1cd6736`, the existing focused suites are green:

```text
cargo test -p agentos-client process::tests
  13 passed; 0 failed
cargo test -p agentos-client stream::tests
  2 passed; 0 failed
cargo test -p agentos-client agent_os::tests::confirmed_shutdown_terminally_clears_all_host_route_collections
  1 passed; 0 failed
```

That is 16 focused tests passing before the new regression is introduced. It
does not prove ordinary terminal callback cleanup; the durable red-before test
above supplies that missing evidence.

## Validation commands

Run focused unit coverage first:

```bash
cargo test -p agentos-client process::tests
cargo test -p agentos-client stream::tests
cargo test -p agentos-client agent_os::tests::confirmed_shutdown_terminally_clears_all_host_route_collections
cargo check -p agentos-client
cargo fmt --all -- --check
```

Then run the real process integration and workspace gate:

```bash
cargo test -p agentos-client --test process_e2e -- --nocapture
cargo check --workspace
git diff --check
```

The E2E test starts a native VM and belongs after the focused tests.

## Dependencies, overlaps, and risks

- **Item 71 owns terminal snapshot expiry.** Item 72 must not infer expiry from
  snapshot absence or add a new protocol lookup. Compact host-route retention
  remains keyed by the sidecar-advertised count until Item 71 defines the
  cross-adapter contract.
- **Item 57 may change exit subscription failures.** Preserve the current typed
  `ProcessExit` in the compact variant so a later result-bearing
  `on_process_exit` API can expose it. Do not deepen the current log-only branch
  as part of Item 72.
- **Item 59 may change finite-exec stdin ownership.** Do not fold its atomic
  finite-input/fail-closed cleanup work into this revision. If it lands first,
  preserve its control failure behavior while changing only public-spawn route
  storage and late terminal lookup.
- **Do not abort normal callback tasks at completion.** Protocol ordering means
  output precedes exit on the pump, but the independent callback task may not
  have run yet. Dropping the handle detaches it; dropping the final senders lets
  it drain and finish safely.
- **Preserve transition races.** A reader must observe either `Running` or
  `Terminal`, never a partly-cleared running value. If it obtains a running
  watch/output receiver immediately before replacement, the terminal event and
  sender closure still carry it to completion.
- **Preserve typed route failures.** A compact failure is not equivalent to a
  successful exit or an unknown PID. Store the exact `ProcessExit` clone used by
  the live watch.
- **Retention zero is valid.** Replacement followed by immediate pruning must
  not strand already-subscribed waiters.
- **Do not remove the global sequence allocator.** Only the per-entry atomic
  sentinel disappears. Completion-order eviction still needs a monotonic
  sequence.
- **Shutdown differs from normal completion.** Aborting tasks is correct for
  authoritative teardown of live routes. Natural sender closure is correct for
  ordered process completion.

## Bounded JJ revision

Implement Item 72 in one dedicated stacked JJ revision, with a conventional
description such as:

```text
refactor(client): compact Rust terminal process routes
```

The intended revision path set is limited to:

- `crates/client/src/agent_os.rs`
- `crates/client/src/process.rs`
- `crates/client/src/stream.rs`
- `crates/client/tests/process_e2e.rs`
- `docs/thin-client-migration.md` only when recording the before/after commands,
  checking all three Item 72 boxes, and marking the row done

No sidecar, protocol, TypeScript, actor, website, or generated mirror file
belongs in this revision. If Item 71 or Item 57 changes the same Rust match sites
first, rebase the dedicated Item 72 revision and preserve their semantics rather
than folding the items together. No secure-exec mirror regeneration or protocol
fixture regeneration is required because no public shim or wire schema changes.
