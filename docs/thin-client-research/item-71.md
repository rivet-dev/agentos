# Item 71 research: make process-history expiry sidecar-authoritative

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 71 tracker status.

Inspected on **2026-07-14** at revision **`905bfc76fb3a`**. Tracker anchors are
`docs/thin-client-migration.md:117` (issue inventory), current line 198
(pending status), and current line 284 (before/after/complete checklist).

## Recommendation

Extend the existing `get_process_snapshot` request/response with an explicit
availability contract:

- a client may include PIDs and sidecar process IDs that it already expects to
  exist in active or retained-terminal state; and
- the sidecar must list every expected key that is no longer available instead
  of forcing the client to infer expiry from its absence in a bulk snapshot.

At the same time, give the browser adapter the same bounded terminal process
history that native already has. Implement the bounded FIFO once in
`agentos-native-sidecar-core` and use it from both adapters. TypeScript and Rust
then forward their locally tracked keys, remove routes only when the response
explicitly marks them unavailable, and reject a malformed response that neither
returns nor classifies an expected key.

Priority: **P1**. Confidence: **high**.

This policy belongs in the sidecar. Only the sidecar knows its active process
table, terminal-history capacity, cross-API churn, and the exact point at which
a terminal record was evicted. The clients retain only bounded host callback and
handle correlation; they must not recreate a second history clock or guess from
snapshot absence.

## Original issue

The tracker entries are at `docs/thin-client-migration.md:117,198,284`:

> Native sidecar process history is shared across `spawn`, `exec`, and shell
> activity, so unrelated churn can evict a public-spawn snapshot while a client
> still retains its terminal route; browser snapshots expose only current
> executions.

There are two independent defects.

### Native history and client route retention count different things

Native VM state declares one `exited_process_snapshots` FIFO in
`crates/native-sidecar/src/state.rs:463,486-489`. Every top-level process exit
pushes into it in
`NativeSidecar::finish_active_process_exit` at
`crates/native-sidecar/src/execution.rs:5631-5664`, regardless of whether the
wire `Execute` originated from finite exec, public spawn, a shell, cron, ACP, or
another internal runtime path.

`prune_exited_process_snapshots` at `execution.rs:12782-12787` bounds that one
FIFO with `process_route_retention(&vm.limits)`. The snapshot handler at
`execution.rs:4321-4348` returns live process trees plus that retained FIFO.

The TypeScript `_processes` map, however, contains only public `AgentOs.spawn`
routes. `_pruneCompletedProcessRoutes` in
`packages/core/src/agent-os.ts:1628-1644` retains that many completed public
routes. Rust does the same for SDK spawn routes in
`crates/client/src/process.rs:1140-1182`.

Therefore 1,024 unrelated finite exec/shell/ACP completions can evict an older
public-spawn snapshot even though the TypeScript/Rust public-spawn map has seen
little or no pressure and still retains that PID. Current clients then produce
a client-invented invariant error:

- TypeScript `listProcesses` / `getProcess` at
  `packages/core/src/agent-os.ts:2035-2055,2089-2109` throw
  `Sidecar process snapshot is missing tracked process`;
- Rust `list_processes` / `get_process` at
  `crates/client/src/process.rs:530-560,647-665` return the analogous
  `ClientError::Sidecar` string.

Those paths cannot distinguish authoritative history eviction from a malformed
sidecar response.

### Browser snapshots discard every terminal process immediately

The browser wire adapter's `ExecutionRecord` and route maps are active-only at
`crates/native-sidecar-browser/src/wire_dispatch.rs:65-105`. When it converts an
`ExecutionEvent::Exited`, lines 2352-2355 remove both maps before emitting
`process_exited`.

`BrowserSidecar::apply_execution_event` in
`crates/native-sidecar-browser/src/service.rs:2658-2687` marks the kernel process
exited and immediately calls `release_execution`, which reaps the kernel process
and drops execution state. Consequently
`BrowserSidecar::process_snapshot_entries` at `service.rs:1614-1630` can return
only active executions. The wire `get_process_snapshot` handler at
`wire_dispatch.rs:1088-1119` has no terminal records to append.

There is one adjacent parity prerequisite in the same browser route: its
`process_started_response` at `wire_dispatch.rs:2094` currently sends
`pid: None`, even though the service's active snapshot already contains the
kernel PID. Both TypeScript and Rust `spawn` reject an execute response without
a PID, so browser late-process and late-shell retention cannot be exercised
through the normal clients until this response forwards the sidecar PID.

This contradicts the browser's advertised `process_route_retention` (1,024 by
default) and breaks late shell behavior. Both TypeScript `waitShell` /
`closeShell` and Rust `wait_shell` / `close_shell` query the sidecar snapshot by
process ID after their live shell route is gone. Native can resolve a retained
exit; browser reports the same known shell as missing immediately after exit.

## Protocol contract

### `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare`

Replace the void request with presence-aware expectations and add explicit
unavailable lists to the existing response:

```bare
type GetProcessSnapshotRequest struct {
  expectedPids: optional<list<u32>>
  expectedProcessIds: optional<list<str>>
}

type ProcessSnapshotResponse struct {
  processes: list<ProcessSnapshotEntry>
  unavailablePids: list<u32>
  unavailableProcessIds: list<str>
}
```

Semantics:

- both expectation fields omitted means an ordinary authoritative diagnostic
  snapshot and both unavailable lists must be empty;
- each present key is a caller assertion that the process was previously
  returned by this VM;
- a key present in active or retained terminal state appears in `processes` and
  not in an unavailable list;
- a key no longer present appears in the matching unavailable list; and
- each expected key must appear exactly once across `processes` and its
  unavailable list.

The response remains a full active-plus-retained snapshot; expectations classify
missing correlation and are not a client-selected history filter. This lets
`allProcesses` preserve its current diagnostic result while `listProcesses`,
`getProcess`, and late shell lookups obtain an explicit expiry signal. Item 41
removes `processTree` earlier in the numbered stack; do not reintroduce it here.

Use optional request fields so ordinary diagnostic callers preserve omission.
Do not make clients fill empty arrays as defaults. Reject duplicate keys,
invalid process IDs, and an expectation count above a sidecar-owned bound. The
bound should be derived in `native-sidecar-core` as:

```text
process_route_retention(limits) + limits.resources.max_processes
```

with checked/saturating arithmetic and the normal default max-process value when
the optional kernel field is absent. The typed limit error must name the
observed count, capacity, and `limits.resources.maxProcesses` as the way to
raise it. The request list is trusted client input but still must not create an
unbounded allocation or quadratic duplicate scan; validate with sets.

Have the shared classifier return a typed error enum, not a message-only
`SidecarCoreError`: invalid or duplicate keys and zero PIDs map to
`invalid_request`, while an expectation count above the derived capacity maps
to `limit_exceeded` in both native and browser adapters. The over-limit variant
must retain observed count, capacity, and the configuration path so neither
adapter has to parse text.

When a request reaches the repository's standard near-limit threshold, emit a
structured `tracing::warn!` from each adapter with the VM ID, observed count,
capacity, and configuration path before classification. Add a shared threshold
predicate test so native and browser warning behavior cannot drift. The hard
failure remains the typed `limit_exceeded` response.

This repository ships protocol, clients, and sidecars in lockstep, so changing
the existing request encoding is acceptable. Do not add compatibility decoding
for the old void payload.

### Generated and live TypeScript shapes

Regenerate `packages/runtime-core/src/generated-protocol.ts` with:

```sh
pnpm --dir packages/build-tools build:protocol
```

Update `packages/runtime-core/src/request-payloads.ts` so
`get_process_snapshot` carries optional `expected_pids` and
`expected_process_ids`, preserving omission as generated `null`. Update
`packages/runtime-core/src/response-payloads.ts` so `process_snapshot` includes
safe-number-converted `unavailable_pids` and exact
`unavailable_process_ids`.

Update the hand-written Rust compatibility conversions in
`crates/sidecar-protocol/src/protocol.rs`. Replace its empty compatibility
`GetProcessSnapshotRequest {}` with the generated wire type alias, copy both
request fields in both conversion directions, and copy both unavailable lists
in `ProcessSnapshotResponse`. Generated Rust remains Cargo build output.

## One shared sidecar-owned history

### Add `crates/native-sidecar-core/src/process_history.rs`

Create a small `ProcessTerminalHistory` around a
`VecDeque<ProcessSnapshotEntry>`. It should be constructed with the resolved
sidecar retention, expose read-only iteration, answer process-ID conflicts, and
push one terminal entry while evicting oldest-first until `len <= capacity`.

Also put the expectation validator/classifier here so native and browser cannot
diverge on duplicate handling, bounds, or unavailable ordering. Its output
should preserve caller expectation order for deterministic wire results.

Re-export the history and classifier from
`crates/native-sidecar-core/src/lib.rs`. Keep the capacity passed in from
`process_route_retention(&limits)`; do not copy the default constant into either
adapter and do not make the clients supply a history size.

Focused shared-core tests must prove:

- capacity is exact and oldest-first for capacities 1 and 2;
- active and retained entries both satisfy expectations;
- missing expected PIDs and process IDs are classified explicitly and in input
  order;
- duplicate/invalid/over-limit expectations are typed failures; and
- the near-limit threshold is shared and reports the observed/capacity values;
- a process ID conflicts while retained but becomes reusable after authoritative
  eviction, preserving native's existing behavior.

### Native adapter edits

In `crates/native-sidecar/src/state.rs`, replace
`exited_process_snapshots: VecDeque<ExitedProcessSnapshot>` and the wrapper
struct with `ProcessTerminalHistory`. Initialize it in
`crates/native-sidecar/src/vm.rs` with the resolved retention.

In `crates/native-sidecar/src/execution.rs`:

- push the existing complete terminal `ProcessSnapshotEntry` through the shared
  history in `finish_active_process_exit`;
- replace direct FIFO iteration/contains/pruning in `snapshot_vm_processes`,
  `vm_has_process_id`, `kill_process_internal_with_source`, and
  `get_process_snapshot` with the shared history methods;
- delete `prune_exited_process_snapshots`; capacity enforcement occurs at the
  sidecar-owned insertion point, not during reads; and
- pass the full snapshot plus request expectations to the shared classifier and
  return both unavailable lists through `process_snapshot_response`.

Keeping pruning at insertion makes the retention invariant continuous and
removes policy work from a read request. Preserve native behavior that killing a
retained terminal process is an idempotent success and that an explicit process
ID cannot be reused until its retained record expires.

### Browser adapter edits

In `crates/native-sidecar-browser/src/wire_dispatch.rs`:

1. Add a VM-keyed `ProcessTerminalHistory` map. Create each history from that
   VM's resolved limits during VM initialization and remove it in
   `purge_vm_state`.
2. Extend `ExecutionRecord` with the complete initial protocol
   `ProcessSnapshotEntry`. After `start_execution_with_options`, obtain the new
   execution's entry through the existing
   `BrowserSidecar::process_snapshot_entries`, replace its bridge execution ID
   with the public wire `process_id`, and store it with the record. Return that
   entry's real PID in `process_started_response` instead of the current `None`.
   Treat a missing just-started entry as `execute_failed`; fail closed by
   releasing the just-started execution and context, preserving cleanup errors,
   rather than orphaning a worker. Do not manufacture PID/process metadata in
   the client.
3. On `ExecutionEvent::Exited`, change the stored entry to `Exited`, set the
   exact exit code and sidecar clock exit time, and push it into the VM's shared
   history before removing active wire routes. Do this for ordinary, cron, and
   internal executions alike so browser/native churn accounting is identical.
4. Append retained history to active entries in `get_process_snapshot`, then use
   the shared expectation classifier and return explicit unavailable lists.
5. Include retained IDs in execute-conflict detection and make kill of a
   retained terminal ID an idempotent success, matching native.

Do not retain workers, contexts, captured-output buffers, ownership envelopes,
or bridge execution handles in terminal history. Store only the compact process
snapshot fields. Browser worker/kernel cleanup must remain immediate.

`crates/native-sidecar-browser/src/service.rs` needs no second history. Its
active execution table should remain active-only; protocol terminal correlation
belongs to the wire sidecar state that owns public process IDs.

### Shared response builder

Change `process_snapshot_response` in
`crates/native-sidecar-core/src/frames.rs` to accept `processes`,
`unavailable_pids`, and `unavailable_process_ids`. Update its native and browser
callers. Do not encode unavailable correlation as `RejectedResponse`; one bulk
snapshot can legitimately contain available and expired expected records at the
same time.

## Thin-client edits

### Runtime-core transport

Change `SidecarProcess.getProcessSnapshot` in
`packages/runtime-core/src/sidecar-process.ts:1653-1674` to accept an optional
expectation object and return:

```ts
interface SidecarProcessSnapshot {
	processes: SidecarProcessSnapshotEntry[];
	unavailablePids: number[];
	unavailableProcessIds: string[];
}
```

Forward only fields the caller supplied. Validate that unavailable values are a
subset of the corresponding expectations, that no expected key is both returned
and unavailable, and that every expected key is classified exactly once as
returned or unavailable. Reject duplicate returned keys and duplicate
unavailable keys as malformed protocol. Do not decide retention or synthesize
unavailable keys in runtime-core.

Update request/response mapping tests in
`packages/runtime-core/tests/request-payloads.test.ts` and
`response-payloads.test.ts`, including omitted, present-empty, mixed available,
and unavailable cases. Add focused `SidecarProcess.getProcessSnapshot` cases in
`packages/runtime-core/tests/sidecar-process.test.ts` for exact-once validation;
mapping tests alone do not exercise the request-correlated invariant.

### TypeScript core

Stack Item 71 after Item 70. Item 70 changes `Kernel.snapshotProcesses` to the
only asynchronous inspection surface; Item 71 should evolve it to accept
optional expectations and return the process array plus explicit unavailable
lists. `AgentOs.allProcesses()` calls it with no expectations and returns only
the response's `processes`.

In `packages/core/src/sidecar/rpc-client.ts`:

- forward expectation fields to `SidecarProcess.getProcessSnapshot`;
- map returned entries without caching them;
- make `processSnapshotById(processId)` send
  `expectedProcessIds: [processId]` and return `undefined` only when the sidecar
  explicitly includes that ID in `unavailableProcessIds`;
- throw a protocol invariant error if the expected ID appears in neither place;
  and
- keep `processSnapshotRefresh` single-flight only for unqualified diagnostic
  snapshots. Expectation-aware requests with different keys must not share an
  in-flight response. Do not introduce a persistent keyed request cache.

In `packages/core/src/agent-os.ts`:

- `listProcesses` sends every locally tracked PID as `expectedPids`, removes
  routes explicitly listed in `unavailablePids`, and returns the remaining
  public processes;
- `getProcess(pid)` sends that expected PID, removes an explicitly unavailable
  route, and throws the existing `Process not found: <pid>` result;
- if a requested PID is neither present nor explicitly unavailable, throw a
  malformed-sidecar invariant error rather than guessing expiry; and
- `waitShell` / `closeShell` retain their existing not-found behavior, but their
  `processSnapshotById` result is now based on the explicit sidecar unavailable
  list rather than raw absence.

The client should not compare its route count with
`processRouteRetention`, count unrelated operations, attach a TTL, or preserve a
terminal fallback after the sidecar reports unavailable.

### Rust client

In `crates/client/src/process.rs`, make the private snapshot helper accept
expected PIDs/process IDs and return a small internal result containing entries
and both unavailable lists.

Validate the same exact-once response invariant at the Rust wire boundary before
reconciling any route. The sidecar returning neither classification, both
classifications, a duplicate, or an unrequested unavailable key is a protocol
error and must not mutate client state.

- `list_processes` sends all local SDK PIDs, removes routes explicitly marked
  unavailable, and maps only the still-authoritative entries;
- `get_process` sends one expected PID, removes it on explicit unavailability,
  and returns the existing `ClientError::ProcessNotFound(pid)`;
- missing-but-not-unavailable is a `ClientError::Sidecar` protocol invariant,
  not expiry; and
- `all_processes` sends no expectations and returns the snapshot entries.

In `crates/client/src/shell.rs`, have
`process_snapshot_entry_by_id` send `expected_process_ids` and return `None` only
for explicit unavailability. `wait_shell` and `close_shell` can keep their
existing `ShellNotFound` public result.

Do not add client terminal sequence numbers or a second Rust/TypeScript history
map. Item 72 may compact Rust's retained host callback routes after this item,
but that state remains separate from authoritative process inspection.

## Before and after tests

### Before evidence

Record these two failures before production edits:

1. In `crates/native-sidecar-browser/tests/wire_dispatch.rs`, execute one
   process, emit and poll its `ExecutionEvent::Exited`, then request a process
   snapshot. Current browser behavior returns no entry for the just-exited
   process despite advertising terminal retention.
2. In the native sidecar service test seam, retain one public-spawn-labelled
   terminal entry, add `DEFAULT_PROCESS_ROUTE_RETENTION` later entries labelled
   as finite exec/shell/ACP churn, then query. Current native behavior evicts the
   first record, while the response has no field that tells a still-retaining
   client that the missing PID is authoritative expiry.

For TypeScript and Rust, add focused client tests first with a snapshot response
that omits a locally retained completed PID. Record the current
`sidecar process snapshot is missing tracked process` errors. Those tests become
the after tests by adding explicit unavailable fields and asserting route
reconciliation.

### Shared history and adapter conformance

Add `crates/native-sidecar-core` unit tests described above. Then add the same
adapter-level scenario to:

- the native service process-snapshot tests in
  `crates/native-sidecar/tests/service.rs`; and
- `crates/native-sidecar-browser/tests/wire_dispatch.rs` next to the existing
  process snapshot test around lines 1309-1331.

For each adapter:

1. an active expected PID/ID is returned and unavailable lists are empty; the
   browser `ProcessStartedResponse` contains that exact nonzero PID;
2. after exit, the same complete entry is returned as `Exited` with exact exit
   code/timestamps;
3. a never-retained expected PID and ID appear in the corresponding unavailable
   lists;
4. mixed available/unavailable expectations preserve request order;
5. after FIFO pressure, the oldest terminal keys become unavailable while the
   newest capacity remains returned; and
6. explicit ID conflict/idempotent terminal kill behavior matches.

Use fabricated/shared-history entries for the 1,025-entry pressure assertion;
do not launch 1,025 real V8/WASM guests. One real adapter execution per backend
is enough to prove lifecycle integration.

### TypeScript client tests

Extend Item 70's
`packages/core/tests/process-snapshot-forwarding.test.ts` to prove exact
expectation forwarding, unavailable conversion, unqualified single-flight, and
non-coalescing expectation-aware requests.

Extend `packages/core/tests/leak-agent-os-processes.test.ts` with a mocked
authoritative snapshot:

- completed local PID + `unavailablePids: [pid]` removes the route and is
  omitted by `listProcesses`;
- `getProcess(pid)` then returns `Process not found`;
- missing PID with an empty unavailable list remains a hard protocol invariant;
  and
- a returned completed entry preserves command/args/exit metadata.

Add a `processSnapshotById` test proving a retained shell entry resolves, an
explicit unavailable ID maps to not-found, and missing-without-classification
rejects.

### Rust client tests

Add pure reconciliation tests beside `process.rs`'s existing terminal-retention
units. Cover a mixed available/unavailable batch, route removal, exact
`ProcessNotFound`, and missing-without-classification. Extend the shell lookup
test seam for retained, unavailable, and malformed responses.

Keep the real native E2Es in `crates/client/tests/process_e2e.rs` and
`shell_e2e.rs`; add one late wait after unrelated lightweight churn if it can
reuse sidecar test commands without launching hundreds of real runtimes. The
shared-core capacity test is the authoritative pressure test.

No client test should implement a retention counter. No sidecar history test
should move into a client suite.

## Dependencies, risks, and non-goals

- **Item 70 should land first.** It removes the stale TypeScript snapshot cache
  and legacy `Kernel.processes` fallback. Item 71 then changes the authoritative
  async snapshot result without preserving either deleted surface.
- **Item 41 lands earlier.** It removes `processTree` from both clients and the
  actor. Keep the protocol response flat and do not restore that derived API.
- **Item 72 should stack after Item 71.** Rust route compaction must preserve the
  process ID/terminal state needed to send expectations and reconcile explicit
  unavailability, but it must not retain sidecar snapshot metadata.
- **Item 32 established late shell lookup.** Preserve its successful native
  behavior and make browser conform; do not restore client-side shell exit
  history.
- **Item 59 may change finite-exec execution shape.** Whether stdin is folded
  into one sidecar operation does not change that every top-level execution
  contributes to the one sidecar terminal-history FIFO.
- **Items 63 and 69 overlap nearby TypeScript code.** Preserve structured
  terminal errors and output-handler isolation. Item 71 owns only snapshot
  availability/history.
- **PID/process-ID reuse:** retained IDs remain conflicts; after authoritative
  eviction native already permits reuse. Clients generate monotonic sidecar IDs
  and do not submit explicit IDs, so normal SDK correlation remains unambiguous.
- **Do not retain output:** terminal history contains metadata and exit status,
  not stdout/stderr. Finite capture already travels on `process_exited`.
- **Do not make bulk snapshot infinite:** it remains bounded by active kernel
  limits plus terminal history. Availability fields do not extend history.
- **Do not return a global `historyTruncated` boolean:** once true, it cannot
  identify which expected route expired and still forces client inference. The
  explicit unavailable key lists are precise for the client-owned correlations.
- **Do not partition history by client API:** the sidecar intentionally sees one
  Linux-like process namespace. Separate spawn/exec/shell quotas would encode
  SDK concepts in runtime policy and would still leave browser divergence.

## Item 74 adjacent defect — do not fold it into Item 71

Item 69's research identified that after a genuine
`NativeSidecarKernelProxy.runEventPump` failure, `pumpError` is set but
`startTrackedProcess` never checks it. A later spawn can therefore start a guest
process when that proxy has no live event consumer, leaving its host wait route
unable to complete.

Tracker Item 74 now owns that defect:

- Item 22 covers bounded/loss-aware existing event subscriptions and fail-closed
  cleanup of a route that loses its stream;
- Item 59 covers post-start stdin/EOF failure cleanup;
- Item 69 covers callback exceptions entering and stopping a healthy pump;
- Item 71 covers terminal snapshot/history availability after process lifecycle
  events; and
- Item 74 rejects starts after a known pump failure and closes the concurrent
  start/failure race.

Do not make Item 71's snapshot lookup or terminal history compensate for a
process started without a live event route. Preserve Item 74's separate bounded
revision and tests.

## Dedicated JJ revision and bounded paths

Implement Item 71 in one dedicated child revision after Item 70. Expected paths:

```text
crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare
crates/sidecar-protocol/src/protocol.rs
crates/native-sidecar-core/src/{lib.rs,frames.rs,limits.rs,process_history.rs}
crates/native-sidecar/src/{state.rs,vm.rs,execution.rs}
crates/native-sidecar/tests/service.rs
crates/native-sidecar-browser/src/wire_dispatch.rs
crates/native-sidecar-browser/tests/wire_dispatch.rs

packages/runtime-core/src/{generated-protocol.ts,request-payloads.ts,response-payloads.ts,sidecar-process.ts}
packages/runtime-core/tests/{request-payloads.test.ts,response-payloads.test.ts,sidecar-process.test.ts}
packages/core/src/{runtime.ts,agent-os.ts}
packages/core/src/sidecar/rpc-client.ts
packages/core/tests/{process-snapshot-forwarding.test.ts,leak-agent-os-processes.test.ts}

crates/client/src/{process.rs,shell.rs}
crates/client/tests/{process_e2e.rs,shell_e2e.rs} # only focused retained-history assertions
docs/thin-client-migration.md                    # status/evidence after validation
```

Generated Rust protocol code remains build output. No package manifest,
lockfile, actor, ACP protocol, website, or public package export should change.
Because other stacked items touch `agent-os.ts`, `rpc-client.ts`, generated
protocol output, and process tests, inspect the exact Item 71 revision paths
before describing/squashing it.

Suggested description:

```text
fix(sidecar): own terminal process expiry
```

## Validation commands

Run generation and focused protocol/core tests first:

```sh
pnpm --dir packages/build-tools build:protocol
cargo test -p agentos-sidecar-protocol
cargo test -p agentos-native-sidecar-core process_history
cargo test -p agentos-native-sidecar --test service process_snapshot
cargo test -p agentos-native-sidecar-browser --test wire_dispatch process_snapshot
```

Then both thin clients:

```sh
pnpm --dir packages/runtime-core exec vitest run \
  tests/request-payloads.test.ts \
  tests/response-payloads.test.ts \
  tests/sidecar-process.test.ts
pnpm --dir packages/core exec vitest run \
  tests/process-snapshot-forwarding.test.ts \
  tests/leak-agent-os-processes.test.ts \
  tests/process-management.test.ts
cargo test -p agentos-client process
cargo test -p agentos-client shell
cargo fmt --all -- --check
cargo check --workspace
pnpm --dir packages/build-tools check:generated
pnpm check-types
git diff --check
```

Item 71 is complete only when native and browser retain the same bounded compact
terminal metadata, mixed API churn evicts through their one shared helper,
responses explicitly classify every expected missing PID/process ID, both
clients reconcile only from that signal, missing-without-classification stays a
hard error, before/after evidence is recorded, and the tracker is marked done
in the dedicated stacked revision.
