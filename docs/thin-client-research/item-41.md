# Item 41 research — remove client-built process trees

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 41 tracker status.

Refreshed against the shared working tree on 2026-07-14. Priority: **P2**.
Confidence: **high**.

## Recommendation

Remove `processTree` / `process_tree` and `ProcessTreeNode` from the TypeScript,
Rust, and actor public APIs. Keep `allProcesses` / `all_processes` as the one
authoritative process-inspection API.

This is preferable to moving the convenience into the sidecar:

- no production caller in this repository uses the tree API; every occurrence is
  its implementation, façade, test, or documentation;
- the sidecar already returns the bounded, authoritative flat kernel snapshot,
  including `pid` and `ppid`, which is the ordinary Linux process-table model;
- the wire protocol has no tree request or response, so retaining the convenience
  would add schema, generated bindings, native/browser dispatch, shared tree
  policy, and client hydration solely to preserve an unused derived view;
- the user explicitly asked not to move unneeded behavior into the sidecar;
- client, sidecar, protocol, and actor package versions ship in lockstep, and this
  repository does not promise protocol backward compatibility.

The public break is intentional: TypeScript callers use `await vm.allProcesses()`
and Rust callers use `os.all_processes().await`; each entry already carries its
kernel `ppid`. Actor callers use the corresponding `allProcesses` action. A caller
that needs presentation-specific indentation can format that flat snapshot at
its UI boundary, just as Linux tools format `/proc`/`ps` data. AgentOS should not
own a second recursive process model.

The tracker currently says medium confidence because move-versus-remove had not
been resolved. Current repository usage, the already-complete flat response, and
the cost of adding an otherwise-unused recursive protocol resolve that ambiguity
in favor of removal.

## Current duplicated behavior

### TypeScript

`packages/core/src/agent-os.ts:2065-2087` calls `allProcesses()`, creates one
mutable node per PID, then attaches each node to the node whose PID matches its
`ppid`. A node becomes a root only when its parent PID is absent.

`allProcesses()` is already sidecar-authoritative on the production path:
`packages/core/src/agent-os.ts:2057-2063` calls the native proxy's
`snapshotProcesses()` method; `packages/core/src/sidecar/rpc-client.ts:747-775`
requests the process snapshot; and `buildProcessSnapshot` at lines 1006-1034
maps and PID-sorts the response. The tree method adds no runtime data.

### Rust

`crates/client/src/process.rs:641-644` calls `all_processes()` and then the private
`build_process_forest` at lines 1080-1125. That helper independently implements
the same roots/children policy and carries a Rust-only `seen` guard. The public
recursive type is declared at lines 170-175 and re-exported from
`crates/client/src/lib.rs:60-63`.

Both clients therefore own these semantics:

- an entry whose `ppid` is absent is a root;
- parent and child order follows the PID-sorted flat snapshot;
- a self-parented entry finds itself as its parent and disappears from the root
  forest;
- a closed parent cycle likewise has no root and disappears;
- the snapshot's parent linkage, not a sidecar tree result, determines shape.

The algorithms agree for valid kernel snapshots but are still two state-shaping
implementations. They are also unnecessary because the flat snapshot is public.

## Existing sidecar and protocol capability

Keep the existing wire surface unchanged:

- `GetProcessSnapshotRequest` is declared at
  `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:409`.
- `ProcessSnapshotEntry` at lines 704-718 carries the authoritative process ID,
  kernel `pid`/`ppid`/`pgid`/`sid`, command, argv, cwd, status, exit code, and
  timestamps.
- `ProcessSnapshotResponse` at lines 720-722 returns a flat bounded list.
- Native dispatch in `crates/native-sidecar/src/execution.rs:4321-4349` checks
  exact VM ownership and `process.inspect`, prunes bounded exited snapshots, and
  returns the VM snapshot.
- Browser dispatch in
  `crates/native-sidecar-browser/src/wire_dispatch.rs:1088-1119` returns the
  browser sidecar's process snapshot through the same response.
- Shared field conversion already lives in
  `crates/native-sidecar-core/src/diagnostics.rs`; both implementations use
  `SharedProcessSnapshotEntry`.

The default kernel process limit is finite (`DEFAULT_MAX_PROCESSES == 256`), and
exited route retention is derived from the resolved process limit. Removing the
tree view neither weakens bounds nor changes inspection permission enforcement.

Do **not** add `GetProcessTreeRequest`, a recursive BARE type, JSON tree payload,
root/child PID graph, or sidecar tree helper for Item 41. Those are all more code
than deleting the unused view. Do not modify snapshot enumeration, PID sorting,
permission checks, or native/browser process coverage in this revision.

## Exact production edits

### TypeScript core

In `packages/core/src/agent-os.ts`:

1. Delete the `ProcessTreeNode` interface at lines 80-83.
2. Delete `AgentOs.processTree()` at lines 2065-2087.
3. Leave `allProcesses()`, `listProcesses()`, and `getProcess()` unchanged.

In `packages/core/src/types.ts`, remove `ProcessTreeNode` from the type re-export
list. No edit is needed in `packages/core/src/index.ts`; it exports `types.ts`.

### Rust client

In `crates/client/src/process.rs`:

1. Change the module-level comment so it names only `exec` and `all_processes`.
2. Delete the public `ProcessTreeNode` struct.
3. Delete `AgentOs::process_tree()`.
4. Delete the private `build_process_forest` function and its behavior comment.

In `crates/client/src/lib.rs`, remove `ProcessTreeNode` from the public process
re-exports. Do not replace it with an alias or deprecated client helper; that
would retain the duplicate behavior Item 41 is meant to remove.

### Rust actor plugin and generated TypeScript actor surface

The actor currently adds a third façade over the Rust client. Remove it in the
same revision so direct and actor APIs remain aligned.

In `crates/agentos-actor-plugin/src/actions/process.rs`:

- remove the `ProcessTreeNode` import;
- delete the `process_tree` helper at lines 116-120.

In `crates/agentos-actor-plugin/src/actions/contract_surface.rs`:

- delete the `processTree` `ActionContract` row at lines 120-124;
- remove `ProcessTreeNode` from the `@rivet-dev/agentos-core` generated type
  imports around line 364.

In `crates/agentos-actor-plugin/src/actions/mod.rs`, remove `processTree` from all
six contract/dispatch sites:

- the contract test-module import of `ProcessTreeNode` around line 389;
- zero-argument decoding around line 434;
- valid sample arguments around line 510;
- invalid sample arguments around line 584;
- sample reply encoding around line 681;
- the production dispatch arm around line 1032.

The list contains six textual edits because reply fixtures and dispatch are
separate even though they belong to the same contract surface.

Regenerate `packages/agentos/src/generated/actor-actions.generated.ts` with
`cargo test -p agentos-actor-plugin --test action_contract`; Cargo runs
`crates/agentos-actor-plugin/build.rs:6-20`, which renders that committed file
from `contract_surface.rs`. The generated diff must remove both the
`ProcessTreeNode` import at current line 8 and the `processTree` action signature
at current line 127. Never hand-edit the generated file as the source of truth.

### Public documentation and repository guidance

Update every active claim found by the repository-wide search:

- `packages/core/README.md:13`: replace “inspect process trees” with “inspect the
  kernel process table” or equivalent.
- `packages/core/README.md:87`: delete the `processTree` API row.
- `packages/core/README.md:167`: delete the `ProcessTreeNode` exported-type row.
- `packages/core/CLAUDE.md:164`: change the target test-layout description from
  “process tree” to “process snapshots.”
- `packages/core/CLAUDE.md:189-191`: describe only `allProcesses()` as derived
  from the sidecar kernel snapshot.
- `website/src/content/docs/docs/architecture/processes.mdx:24` and
  `website/public/docs/docs/architecture/processes.md:22`: change “system-wide
  views (`allProcesses`, `processTree`)” to the single system-wide
  `allProcesses` snapshot. Keep the explanation that guest-created children are
  included.
- `docs/thin-client-research/item-70.md:20,116,237`: remove `processTree` from
  the later process-cache plan and its flow diagram; Item 70 should preserve
  only `allProcesses`, `listProcesses`, and `getProcess` after Item 41 lands.
- `docs/thin-client-research/item-71.md:132`: remove `processTree` from the later
  process-correlation plan so it names only `allProcesses` as the unchanged
  diagnostic view.

No root README, protocol documentation, or secure-exec mirror file refers to
this API. The generated compatibility mirror does not shim `packages/core` or
`crates/client`, so mirror regeneration is not required for Item 41.

## Before validation

The current integration tests prove ordinary root/child behavior but do not
isolate the duplicated orphan/self-parent/order contract named by the tracker.
Before deleting the API, add temporary characterization cases and run them
against the Item 40 parent.

### TypeScript characterization

In `packages/core/tests/process-tree.test.ts`, create an object with
`AgentOs.prototype` and stub only `allProcesses()` to return a PID-sorted fixture:

- PID 2 with missing parent 99;
- PID 3 parented to 2;
- PID 4 parented to itself;
- PID 5 parented to 3;
- PID 6 as a second orphan to prove root order;
- PID 7 as the second child of PID 2 to prove child order.

Assert that PID 2 is a root, 3 and 5 retain nested order, PID 4 is absent, and
roots remain PID ordered. This calls the actual TypeScript method without
booting a VM.

Add this temporary case (plus the `ProcessInfo` type import), run it on the Item
40 parent, and record the result before deleting the file:

```ts
import type { ProcessInfo } from "../src/runtime.js";

const processInfo = (pid: number, ppid: number): ProcessInfo => ({
	pid,
	ppid,
	pgid: pid,
	sid: 1,
	driver: "test",
	command: `process-${pid}`,
	args: [`process-${pid}`],
	cwd: "/workspace",
	status: "running",
	exitCode: null,
	startTime: pid,
	exitTime: null,
});

test("preserves orphan, self-parent, nested-child, and PID order policy", async () => {
	const vm = Object.create(AgentOs.prototype) as AgentOs;
	vm.allProcesses = async () => [
		processInfo(2, 99),
		processInfo(3, 2),
		processInfo(4, 4),
		processInfo(5, 3),
		processInfo(6, 99),
		processInfo(7, 2),
	];

	const roots = await vm.processTree();
	const flatten = (nodes: typeof roots): number[] =>
		nodes.flatMap((node) => [node.pid, ...flatten(node.children)]);
	expect(roots.map((node) => node.pid)).toEqual([2, 6]);
	expect(roots[0]?.children.map((node) => node.pid)).toEqual([3, 7]);
	expect(roots[0]?.children[0]?.children.map((node) => node.pid)).toEqual([5]);
	expect(flatten(roots)).toEqual([2, 3, 5, 7, 6]);
});
```

### Rust characterization

In the private `#[cfg(test)]` module in `crates/client/src/process.rs`, temporarily
import `build_process_forest`, construct the same `ProcessInfo` fixture, and
assert the same root, nested-child, self-parent omission, and order behavior.

Use this exact temporary test shape:

```rust
fn process_info(pid: u32, ppid: u32) -> ProcessInfo {
    ProcessInfo {
        pid,
        ppid,
        pgid: pid,
        sid: 1,
        driver: String::from("test"),
        command: format!("process-{pid}"),
        args: vec![format!("process-{pid}")],
        cwd: String::from("/workspace"),
        status: ProcessStatus::Running,
        exit_code: None,
        start_time: f64::from(pid),
        exit_time: None,
    }
}

#[test]
fn process_forest_preserves_orphan_self_parent_and_order_policy() {
    let roots = build_process_forest(vec![
        process_info(2, 99),
        process_info(3, 2),
        process_info(4, 4),
        process_info(5, 3),
        process_info(6, 99),
        process_info(7, 2),
    ]);

    assert_eq!(roots.iter().map(|node| node.info.pid).collect::<Vec<_>>(), vec![2, 6]);
    assert_eq!(roots[0].children.iter().map(|node| node.info.pid).collect::<Vec<_>>(), vec![3, 7]);
    assert_eq!(roots[0].children[0].children[0].info.pid, 5);

    fn flatten(nodes: &[ProcessTreeNode]) -> Vec<u32> {
        nodes
            .iter()
            .flat_map(|node| {
                let mut pids = vec![node.info.pid];
                pids.extend(flatten(&node.children));
                pids
            })
            .collect()
    }
    assert_eq!(flatten(&roots), vec![2, 3, 5, 7, 6]);
}
```

Extend the existing `use super::{...}` list with `build_process_forest`,
`ProcessInfo`, `ProcessStatus`, and `ProcessTreeNode` while this test exists.

Run and record both passing commands in the tracking checklist:

```sh
pnpm --dir packages/core exec vitest run tests/process-tree.test.ts
cargo test -p agentos-client process_forest
```

These are historical before-evidence only. Delete the temporary pure
characterization tests together with the API; retaining a private tree builder
solely for a test would defeat the removal.

## After validation

### TypeScript core

Delete `packages/core/tests/process-tree.test.ts`. Its real parent/child coverage
is already present in `packages/core/tests/all-processes.test.ts:43-110`, which
asserts the flat snapshot's authoritative `ppid` and proves a guest
`child_process.spawn` child appears with its SDK-spawned parent PID.

Strengthen `packages/core/tests/public-api-exports.test.ts` with explicit removal
coverage by asserting `"processTree" in AgentOs.prototype` is false. After the
package build, inspect the emitted declarations to prove the recursive type is
gone as well; do not retain a source-level reference to the removed type solely
as a negative test.

Place this assertion in the existing root value-surface test after the
`AgentOs` function assertion:

```ts
expect("processTree" in AgentOs.prototype).toBe(false);
```

Run:

```sh
pnpm --dir packages/core exec vitest run \
  tests/all-processes.test.ts tests/public-api-exports.test.ts
pnpm --dir packages/core check-types
pnpm --dir packages/core build
! rg -n 'ProcessTreeNode|processTree' packages/core/dist
```

### Rust client

In `crates/client/tests/process_e2e.rs`:

- remove `process_tree` from the module documentation;
- remove the call/assertion block at current lines 88-92;
- at current lines 267-297, rename the section to `kernel snapshot:
  all_processes`, delete `tree`, the root-count assertion, and the final
  `for root in &tree` loop;
- retain the real `all_processes()` calls and assertions that the spawned kernel
  PID, command, args, cwd, timestamps, and flat snapshot data are authoritative.

Then run:

```sh
cargo test -p agentos-client --lib
cargo test -p agentos-client --test process_e2e
cargo check -p agentos-client
```

The real E2E still requires the native sidecar and registry command artifacts,
under its existing environment/skip policy. Do not turn a missing binary into a
new silent skip while doing this item.

### Actor parity and generated contract

The existing contract suite already requires dispatcher arms, contract rows,
argument fixtures, reply fixtures, and generated TypeScript signatures to agree.
Run it after regenerating the committed TypeScript surface, then verify the
generated declarations no longer contain the removed action:

```sh
cargo test -p agentos-actor-plugin --test action_contract
cargo check -p agentos-actor-plugin
pnpm --dir packages/agentos check-types
pnpm --dir packages/agentos test
! rg -n 'ProcessTreeNode|processTree' \
  packages/agentos/src/generated/actor-actions.generated.ts
```

This is the relevant client/actor parity proof for the removal path. Sidecar
tree tests are intentionally not added because no sidecar tree API is being
created. Existing native/browser `GetProcessSnapshot` tests remain the runtime
authority tests:

- `crates/native-sidecar/tests/service.rs:3057-3128` covers inspection
  permission rejection and an authorized process snapshot;
- `crates/native-sidecar-browser/tests/wire_dispatch.rs:1309-1331` covers the
  browser snapshot response and authoritative PID/cwd fields.

Run those suites if surrounding changes disturb snapshot behavior; the planned
removal should not touch them.

### CI impact

No CI workflow or `scripts/ci.sh` edit belongs in Item 41.

- `.github/workflows/ci.yml:30-47` builds and typechecks the TypeScript workspace,
  so a stale generated actor file that still imports removed `ProcessTreeNode`
  fails the regular checks job.
- `.github/workflows/ci.yml:115-120` runs workspace-wide Rust clippy and the full
  `agentos-client` test package, covering the Rust deletion and retained flat
  process E2E.
- `.github/workflows/ci.yml:126-131` runs `pnpm test`, including the Core and
  actor package suites.
- `.github/workflows/ci-nightly.yml:34` runs `cargo test --workspace`, which
  includes `crates/agentos-actor-plugin/tests/action_contract.rs` and permanently
  checks dispatcher/contract/generated-signature parity. Item 40 separately
  adds a regular-CI actor persistence invocation; Item 41 must not broaden that
  unrelated persistence command.

The focused `cargo test -p agentos-actor-plugin --test action_contract` command
remains mandatory before sealing Item 41 even though the same test is currently
nightly rather than a separate regular-CI step. Adding another workflow command
would be CI expansion, not required process-tree behavior.

### Documentation and final gates

```sh
pnpm --dir website build
cargo fmt --check
git diff --check
rg -n 'ProcessTreeNode|processTree|process_tree' \
  packages/core crates/client crates/agentos-actor-plugin packages/agentos \
  website/src/content/docs website/public/docs \
  docs/thin-client-research/item-70.md docs/thin-client-research/item-71.md
```

The final `rg` must return no product/API occurrences. Generic internal phrases
such as native process termination helpers named “process tree” are unrelated
and should not be renamed.

Update Item 41's acceptance row in `docs/thin-client-migration.md` to reflect the
chosen removal path: the before checkbox cites the two characterization tests;
replace the current “Sidecar tree tests” wording with removal guards plus
authoritative flat-snapshot tests; the work-item confidence becomes high; and
the completion checkbox is marked only after the dedicated revision and all
focused gates pass.

## Public API and migration implications

- **TypeScript core:** removes `AgentOs.processTree()` and the exported
  `ProcessTreeNode` type.
- **Rust client:** removes `AgentOs::process_tree()` and the exported
  `ProcessTreeNode` struct.
- **Actor API:** removes the `processTree` action and generated client method.
- **Wire protocol:** unchanged; `GetProcessSnapshotRequest` remains available to
  both clients through `allProcesses` / `all_processes`.
- **Permissions:** unchanged; `allProcesses` continues to require the sidecar's
  `process.inspect` permission decision.
- **Migration:** callers consume the flat list and read each entry's `ppid`.
  Do not publish a replacement tree helper from another AgentOS package.

An older generated actor client calling `processTree` against a new actor plugin
will receive the ordinary unknown-action error. That is acceptable only because
the actor/plugin/client artifacts are released together; call out the removal in
release notes even though no compatibility shim should be added.

## Risks and dependencies

- **Stack predecessor:** implement only after Item 40 is sealed, as required by
  the one-item-per-revision stack. Item 40 owns actor persistence CI; do not mix
  its workflow changes into Item 41.
- **Item 55 path overlap:** it also changes the hand-maintained API inventory in
  `packages/core/README.md`. Land/rebase carefully so `processTree` is not
  regenerated into the inventory.
- **Items 70 and 71 assume this API remains:** their current research mentions
  `processTree` in later process snapshot/cache work. Update those planning notes
  in this revision so their implementations do not restore or special-case the
  deleted surface.
- **Generated actor file:** changing only the generated TypeScript file will be
  overwritten by `build.rs`; `contract_surface.rs` is authoritative.
- **Contract match tables:** `processTree` appears in decoding, valid fixtures,
  invalid fixtures, sample replies, and dispatch. Missing one will fail the
  action contract suite, which is desirable.
- **Tests are not production users:** do not interpret the two current process
  tree integration tests as evidence that the API is required. Their meaningful
  parent/child assertion already exists on `allProcesses`.
- **Unrelated process-tree terminology:** native sidecar functions such as
  `terminate_child_process_tree` implement required kill semantics and must
  remain. Item 41 concerns only the public derived snapshot view.
- **No protocol regeneration:** removal does not edit the BARE schema. If an
  implementation starts changing generated protocol files, it has drifted onto
  the rejected move path.

## Rejected move alternative

If a future concrete consumer requires a sidecar tree, implement it as a new
sidecar-owned graph, not by restoring the client algorithms. A viable typed BARE
shape would return ordered root PIDs plus ordered child PID lists referencing the
existing process entries; a shared helper in `native-sidecar-core::diagnostics`
would classify roots/cycles once, and native/browser dispatchers would call it.
Clients would only hydrate that trusted graph into their language shape.

That fallback would require protocol schema/generated-code updates, native and
browser dispatcher cases, shared root/orphan/cycle/order tests, TypeScript/Rust
deserializers, actor parity, and explicit response-size bounds. None of that is
justified without a real consumer, so it is outside Item 41.

## Dedicated stacked `jj` revision

Create one revision on top of completed Item 40, keeping the existing stack
bookmark and working-copy branch. Suggested description:

```text
refactor(client): remove derived process tree APIs
```

Intended path scope:

- `packages/core/src/agent-os.ts`
- `packages/core/src/types.ts`
- `packages/core/tests/process-tree.test.ts` (delete)
- `packages/core/tests/all-processes.test.ts` (only if strengthening flat
  snapshot coverage is necessary)
- `packages/core/tests/public-api-exports.test.ts`
- `packages/core/README.md`
- `packages/core/CLAUDE.md`
- `crates/client/src/process.rs`
- `crates/client/src/lib.rs`
- `crates/client/tests/process_e2e.rs`
- `crates/agentos-actor-plugin/src/actions/process.rs`
- `crates/agentos-actor-plugin/src/actions/contract_surface.rs`
- `crates/agentos-actor-plugin/src/actions/mod.rs`
- `packages/agentos/src/generated/actor-actions.generated.ts`
- `website/src/content/docs/docs/architecture/processes.mdx`
- `website/public/docs/docs/architecture/processes.md`
- `docs/thin-client-research/item-70.md`
- `docs/thin-client-research/item-71.md`
- `docs/thin-client-migration.md`

No sidecar, browser sidecar, protocol, CI workflow/script, lockfile, or
secure-exec mirror path should appear in this revision. Verify `pwd` and
`jj log -r @` before creating it; after validation, describe the revision and
advance only the existing stack bookmark.
