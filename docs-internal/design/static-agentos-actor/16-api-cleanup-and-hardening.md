# 16: agentOS 0.4 API Cleanup and Hardening

**Status:** Implementation in progress; not ready to merge

## Purpose

Finish the public hosted actor contract after the static Rust actor migration.
The actor remains a thin RivetKit wrapper over agentOS Core. It owns durable
actor configuration, lifecycle reconciliation, hosted-only policy, RivetKit
cron integration, package warming, and translation between public actions and
Core. The sidecar remains the single owner of VM defaults and runtime behavior.

There is no storage or API backward-compatibility requirement for this work.
The Rust actor, generated TypeScript contract, examples, integrations, and docs
move together.

## Locked decisions

- The hosted actor is named `agentOS` and clients use RivetKit's ordinary actor
  handle with generated agentOS types. There is no custom client wrapper.
- Public guest-environment terminology is **VM**. Public actor actions and
  events use `vm.*`, while internal Rust implementation names may continue to
  use `RuntimeController` where that describes the implementation.
- Creation config is used only when RivetKit first creates an actor. A later
  `getOrCreate` input does not reconfigure an existing actor.
- Config mutation exposes only `config.set` and `config.patch`.
- `config.patch` uses RFC 7386 JSON Merge Patch. RFC 6902 path operations and
  Kubernetes strategic/server-side apply are not exposed.
- Cron stays internal to the hosted actor and uses RivetKit's durable scheduler.
- Network response streaming remains bounded pull-based for this version.
- The hosted actor never accepts host filesystem paths or host mounts.
- The hosted actor defaults to an ephemeral VM root. Durable filesystems are
  explicit whitelisted `durable` roots or mounts; actor state itself uses
  the RivetKit SQLite adapter independently of that filesystem choice.
- Actor-owned, Core-owned, and filesystem-owned schemas share Rivet SQLite but
  retain independent migration tables and namespaces.
- The local SQLite proof-of-concept is a merge blocker. The hosted actor must
  use the production Rivet SQLite adapter before this stack is ready.
- Existing inspector capabilities that still apply are ported as an independent
  consumer of the public actor actions and events.

## Public configuration contract

### Actions

| Action | Meaning |
| --- | --- |
| `config.get` | Return the normalized desired config and revision state. |
| `config.set` | Replace the entire config document. Omitted fields select defaults. |
| `config.patch` | Apply an RFC 7386 merge patch to the current desired config. |

Both mutations accept `expectedRevision`. A stale conditional mutation fails
without modifying durable state. All config-changing paths, including software
install and uninstall, go through one serialized mutation primitive:

```text
mutate_config(expected_revision, mutation)
  -> load current normalized desired config
  -> verify revision
  -> apply replace, merge patch, or typed mutation
  -> normalize and validate with the sidecar-owned policy
  -> resolve remote package sources
  -> persist one next revision
  -> classify live versus restart-required changes
  -> reconcile actor and VM status
```

`config.set` treats its input as a complete input document. Every missing field
is reset to its default. `config.patch` follows these rules:

- A missing object member is unchanged.
- A non-null value replaces the member.
- An object value recursively merges into an object member.
- `null` removes the member from the input document, which selects the default
  when the document is normalized.
- Arrays replace atomically. They are never merged by index or identity.
- Unknown fields fail validation.

For example, patching `{ "environment": { "DEBUG": "1" } }` preserves other
environment keys because `environment` is an object. Patching
`{ "software": [...] }` replaces the whole software array. Patching
`{ "limits": null }` resets limits to the sidecar defaults.

### Why this differs from Kubernetes

Kubernetes supports JSON Merge Patch, JSON Patch, strategic merge patch, and
server-side apply because many independent field managers mutate large shared
resources. agentOS has one trusted config owner and revision-checked writes.
RFC 7386 plus full replacement covers the useful cases without path escaping,
array merge keys, field ownership, or conflict metadata.

### Defaults and desired state

The sidecar owns VM defaults and runtime validation. Actor and embedded clients
do not copy sidecar default constants; some empty-list fields are sent
explicitly. The actor normalizes its hosted-only shape and persists the desired
document without materializing sidecar defaults. For a running VM, restart
classification asks the serving sidecar to compare resolved VM creation policy
for an explicit whitelist of runtime fields. Software sources, hosted
filesystem policy, and any future unmapped fields remain actor-owned
replacement-triggering comparisons.

`config.get` reports desired configuration. `vm.status` separately reports the
running generation, applied revision, readiness, package warming, and issues.
A valid desired update may therefore be durable while the current VM still
reports `restartRequired`.

## VM lifecycle terminology

The public actor surface uses:

- `vm.status`
- `vm.restart`
- `vm.booted`
- `vm.shutdown`
- `vm.limitWarning`

Examples, integrations, generated types, and docs use VM for the guest
environment. Existing package names or third-party APIs that already contain
`sandbox` are not renamed by this step.

## Process and terminal API

### Simplified process surface

| Current action | 0.4 action | Decision |
| --- | --- | --- |
| `process.exec` | removed | Shell-string execution is ambiguous and injection-prone. |
| `process.execFile` | `process.run` | Structured executable plus argv, with captured output. |
| `process.spawn` | unchanged | Structured executable plus argv, returning a live process. |
| `process.kill` | removed | Use `process.signal` with `SIGKILL`. |
| `process.readOutput` | `process.output.read` | Nest output replay under the output resource. |
| `process.writeStdin` | `process.stdin.write` | Nest stdin mutation. |
| `process.closeStdin` | `process.stdin.close` | Nest stdin mutation. |
| `process.resizePty` | `process.pty.resize` | Nest PTY mutation. |

`exec` and `execFile` are inherited Node.js terms with a dangerous distinction:
`exec` commonly means shell parsing while `execFile` commonly means direct argv
execution. That distinction is easy for both people and generated code to miss.
It also duplicates `spawn`, encourages concatenated shell strings, and makes
quoting platform-dependent. `run` is the captured structured operation;
`spawn` is the live structured operation. A caller that intentionally needs a
shell runs `sh` with explicit arguments.

The complete process groups are:

```text
process.run
process.spawn
process.get
process.list
process.tree
process.wait
process.signal
process.stdin.write
process.stdin.close
process.output.read
process.pty.resize

terminal.open
terminal.list
terminal.wait
terminal.close
terminal.stdin.write
terminal.output.read
terminal.pty.resize
```

### Unified replay

Process and terminal output use the same bounded sequenced replay model:

- monotonically increasing cursor within one VM generation;
- `after`, `maxEvents`, and `maxBytes` request bounds;
- a response containing events, next cursor, truncation/gap state, and terminal
  end state;
- events are hints for low latency, while replay is authoritative for recovery;
- cursors from a stale VM generation fail with a typed `stale_generation` error.

The event names are `process.output`, `process.exit`, `terminal.output`, and
`terminal.exit`. Separate terminal stdout/stderr event schemas should only exist
if the PTY implementation can actually preserve that distinction.

## Pull and event inventory

### Pull-based APIs

- `process.output.read`
- `terminal.output.read`
- `network.fetchStream.read`
- `process.wait`
- `terminal.wait`
- all `get`, `list`, `status`, `tree`, snapshot, filesystem read, and config
  observation actions

`network.fetchStream.read` is a bounded blocking pull, not a log poll. It waits
up to a configured duration for the next response chunk and returns data, EOF,
or timeout. This avoids one actor event per network chunk and gives callers
explicit backpressure and cancellation.

### Events

- `vm.booted`
- `vm.shutdown`
- `vm.limitWarning`
- `process.output`
- `process.exit`
- `terminal.output`
- `terminal.exit`
- `cron.fired`

Events provide low-latency notifications. Durable or bounded pull APIs remain
the recovery path after disconnects, dropped subscriptions, or inspector
reloads.

## Cross-surface consistency

The implementation must also standardize:

- every action input as an object, including empty input objects;
- identifier field names and handle shapes across get/list/wait/mutation calls;
- one cursor/page envelope for bounded list and replay results;
- explicit byte and millisecond unit suffixes;
- one structured exit status across process, terminal, cron, and events;
- one mutation result envelope for config and software changes;
- one typed error taxonomy, including `invalid_input`, `limit_exceeded`,
  `revision_conflict`, `stale_generation`, `not_found`, and `not_ready`;
- shared network request/response DTOs where `network.fetch` and embedded
  `httpRequest` have equivalent semantics;
- the distinction between desired config and applied VM status.

Language namespaces remain separate because their workflows are intentionally
different across JavaScript, TypeScript, and Python.

## Contract generation and compatibility

The agentOS bindgen remains a product-local prototype. It exports Rust DTOs,
the action/event registry, and contract metadata, then generates types for a
normal RivetKit actor handle. RivetKit itself is not modified by this step.

The exported contract includes:

- `apiVersion`, initially `agentos-sdk.dev/v1alpha1`;
- `contractMajor`, initially `1`;
- `contractHash`, derived deterministically from the canonical public contract;
- `capabilities`, including pull streaming, config merge patch, inspector
  resources, and supported language namespaces.

During alpha, actor and generated package ship in lockstep and may break
together. After `v1`, additions are compatible within major 1 and breaking
changes require major 2. If two majors must overlap, register versioned actor
implementations internally. Do not add a REST facade only for versioning.

Future native RivetKit action streaming can back a newer client API while the
actor keeps the existing pull actions for a deprecation window. Transport
improvements therefore do not force an immediate public contract break.

## Actor action limits and large payloads

The actor documentation must state, in one table, the effective limits for:

- encoded action request and response size;
- single and batch filesystem reads/writes;
- recursive directory listing and export;
- network request/response bodies and stream chunk size;
- process argv, environment, stdin, captured output, replay bytes, and wait
  durations;
- terminal input, replay, and lifetime;
- language source, files, values, dependency requests, and results;
- software package count, compressed/uncompressed bytes, download duration,
  redirects, and cache capacity;
- config size, action concurrency, previews, cron jobs, and event retention.

Every actor action stays below the RivetKit action payload limit. Batch methods
must account for the whole encoded envelope, not only individual entries. Large
files and exports remain bounded for 0.4; a shared backpressured streaming
transport is a separate design rather than a larger action limit.

## Software installation, cache, and preloading

The hosted actor accepts only remote URL package sources. Embedded Core may
also accept a trusted local path. Both resolve to an immutable `.aospkg`
identity before projection under `/opt/agentos`.

`software.list` reports packages currently installed in the running VM.
`config.get` reports desired package sources, even when the VM is unavailable.
`software.install` and `software.uninstall` are typed config
mutations and use the same revision, normalization, persistence, and
reconciliation path as `config.set` and `config.patch`.

The process cache uses digest identity, single-flight downloads, a byte-bounded
LRU, generation pins, temp-file cleanup, and bounded usage observations. A
central preload coordinator stores only a low-frequency popularity hint. Each
process reads it once at startup and coalesces refreshes per process.

Required configured packages must resolve before the VM becomes ready.
Optional advisory preloading runs under a fixed timeout. At timeout:

- completed cache entries remain available;
- optional downloads with no required waiter are cancelled and partial temp
  files are removed;
- required downloads continue to determine readiness or fail startup;
- the VM continues startup once required packages are ready;
- timeout counts and affected package identities are logged and measured.

Production completion also requires persistent cache recovery, a minimum free
disk reserve, crash cleanup, cancellation tests, cache/preload metrics,
coordinator deployment, and capacity tuning.

## Rivet SQLite and schema ownership

The hosted actor uses the RivetKit SQLite API, never an actor-local SQLite file.
The per-VM database is physically shared while schemas remain independently
owned:

- filesystem: `agentos_fs_*` and `agentos_fs_schema_version`;
- sidecar/Core: `agentos_core_*` and `agentos_core_schema_version`;
- static actor: `agentos_actor_*` and `agentos_actor_schema_version`.

Every owner has an append-only migration ladder and updates its version in the
same transaction as its schema mutation. All agentOS-owned tables are
`STRICT`. No owner reads or advances another owner's migration table. The old
shared component-version mechanism and local hosted runtime database are
removed without compatibility aliases or dual writes.

## Cron correctness

Cron remains a hosted-actor concern. Public actions validate and serialize a
bounded `process.spawn` descriptor into RivetKit's durable scheduler. The
private invoke action is absent from generated public types. Invocation checks
the config revision, launches through the same process path as the public
action, and emits one bounded `cron.fired` event with either a process handle or
a typed launch error. The actor does not add a second parser, wakeup loop, or
cron database.

## Inspector

Port the applicable inspector views as a separate consumer of the generated
actor contract:

- filesystem browser and mount inspection;
- process tree, status, output replay, signals, and stdin;
- terminals and terminal replay;
- installed software and package state;
- desired config, VM status, issues, limits, warnings, and restart;
- previews and networking status.

The tabs may reuse existing UI components, but their data access moves to the
public actor actions/events. Do not port agents, sessions, ACP, prompts,
adapters, TypeScript actor hooks, host mount editing, or private actor internals.

## Documentation

Update existing quickstart, filesystem, process, network, software, resource
limit, permission, persistence, architecture, Rivet Actor, Eve, and Flue docs.
Add or consolidate pages for:

- hosted actor API limits;
- pull transport, events, replay, and future streaming;
- configuration replacement, merge patch, defaults, and revision conflicts;
- software installation, cache, and preload architecture;
- contract versioning and generated RivetKit types;
- VM lifecycle, generations, status, and restart;
- hosted actor versus embedded Core;
- agentOS 0.4 migration;
- inspector capabilities.

## Implementation checklist

### Contract and configuration

- [x] Add `config.patch` with RFC 7386 semantics and focused object, null, and
      array tests.
- [x] Route set, patch, install, and uninstall through one revision-checked
      mutation engine.
- [ ] Move shared defaults, validation, normalization, and change
      classification to one sidecar-owned implementation.
      The serving sidecar now compares resolved VM creation policy for
      whitelisted fields on running VMs; actor-owned software/filesystem policy
      and future fields remain local. Full normalization
      ownership and no-running-VM comparison still need consolidation.
- [x] Keep generated clients free of copied runtime defaults.
- [x] Keep `config.get` desired state distinct from `vm.status` applied state.

### Names and action shape

- [x] Rename public `runtime.*` actions and events to `vm.*`.
- [x] Replace `process.execFile` with `process.run` and remove `process.exec`.
- [x] Nest process stdin, output, and PTY actions.
- [x] Align terminal stdin, output, and PTY action names with process actions.
- [ ] Standardize all action inputs, IDs, units, result envelopes, and typed
      errors. Process `startedAt` is now `startedAtMs`, process tree timestamps
      are `startTimeMs`/`exitTimeMs` with a structured `exit` and an optional
      controllable process handle. Process forests now use raw kernel identity
      internally so display-PID collisions cannot attach children to the wrong
      actor handle; inspector expansion uses handle identity where available.
      Hosted file metadata, installed software, and resolved package sources
      use `sizeBytes`; list envelopes and remaining cross-action consistency
      still need an audit. Expired fetch-stream handles and required package
      resolution deadlines now map to typed `expired` and `timeout` errors.
      Process argv now preserves valid empty elements; empty commands/CWDs are
      `invalid_input`, while oversized strings are `limit_exceeded`. Terminal
      CWDs use the same validation; DTO round trips and argument count/UTF-8
      byte boundaries have focused regression coverage. The sidecar preserves
      an empty `sh -c` script when adding a non-root CWD prefix, without
      dropping empty positional arguments.
- [x] Regenerate the TypeScript contract and update type tests, examples, and
      integrations.

### Replay, transport, and limits

- [x] Implement one generation-scoped replay envelope for process and terminal
      output.
- [x] Preserve bounded pull semantics for network response streams.
- [x] Document actor payload, time, concurrency, retention, and resource
      limits in the hosted-actor limits reference.
- [x] Test aggregate batch/action bounds and stale-generation cursors.

### Compatibility metadata

- [x] Export `apiVersion`, `contractMajor`, deterministic `contractHash`, and
      capabilities.
- [x] Exclude private actions from the public type tree and hash.
- [x] Add deterministic-generation and compatibility-policy tests.

### Storage and scheduling

- [x] Replace every hosted local SQLite path with the production Rivet SQLite
      adapter.
- [x] Verify the three independent strict schema namespaces and transactional
      migrations.
- [x] Remove temporary SQLite wording, configuration, and follow-up items.
- [x] Verify cron uses RivetKit durability only and private invocation is
      revision-safe, bounded, observable, and absent from public bindings.

### Trusted VM configuration

- [x] Keep guest permissions active while trusted creation, mount updates,
      package projection, and host-binding registration run. Operator-only
      kernel paths preserve mount and command validation, bounded command
      discovery, and empty mountpoint cleanup without exposing a guest-callable
      bypass. Focused tests cover guest denial, mount errors, directory limits,
      failed command-stub publication, and preservation of profile commands.
- [x] On a failed final permission write, restore the original policy or
      fail closed to deny-all, and keep the stored VM policy in sync.

### Software and preloading

- [x] Route software mutations through the config mutation engine.
- [x] Verify cache single-flight, byte LRU, pins, cleanup, and cancellation.
- [x] Implement and test optional preload timeout behavior, including dropping
      actor waiters and reporting pending package digests. Already-sent sidecar
      requests use their remaining-time deadline; immediate cancellation of a
      disconnected client waiter remains open below.
- [x] Add persistent cache recovery, free-disk reserve, crash cleanup, and
      startup capacity controls; default to an isolated process-lifetime cache
      when no per-worker persistent directory is configured. Standalone workers
      clean these directories on normal shutdown or startup failure; forced
      termination and host-crash leftovers require host temporary cleanup.
- [x] Export cache/preload metrics with fixed, bounded labels and worker-level
      sampling.
- [ ] Verify coordinator deployment and capacity tuning with multiple actor
      worker processes, including metrics-route access controls.
- [x] Document URL install, desired versus installed state, cache limits,
      readiness, preload timeout, and failures.

### Inspector and documentation

- [x] Locate and port reusable filesystem, process, terminal, software, VM,
      preview, and limits inspector tabs to the generated actor contract.
      Terminal reattachment now pages within the actor's 256-event replay cap,
      bounds the total repaint, and preserves sequence zero when the initial
      snapshot is empty. Independent review preserved bigint cursors, detects
      truncation/gaps, and deduplicates late live events. The process tab now
      drains up to eight replay pages per poll and defers its exit marker until
      backlog is consumed. Independent review bounds decoded page bytes, stops
      drains on detail disposal, pins each drain to one actor, and preserves
      UTF-8 across pages/polls and separate stdout/stderr streams. Terminal list
      failures are visible without the removed legacy boot gate. Terminal IDs
      are scoped to the captured actor handle across lookup and dispatch (33
      focused inspector tests pass, including actor switches and the shared
      filesystem query-key test).
- [x] Remove agent/session/ACP and host-mount inspector surfaces.
- [x] Update public docs and navigation for the 0.4 actor API.
- [x] Ensure public docs consistently call guest environments VMs.

### Validation

- [x] Run actor unit tests and bindgen snapshot/type tests.
- [x] Run `cargo check --workspace` after the package-mount review fixes;
      rerun after the process-snapshot, timer-wheel, and embedded process
      lifecycle fixes. Warnings are pre-existing unused internal helpers.
- [x] Regenerate and check both Rust-derived TypeScript contracts; run the
      bindgen tests (3/3), `pnpm build` (50/50), and `pnpm check-types`
      (103/103) after the new limit field and again after the lifecycle fixes.
- [x] Run the limit inventory audit (5/5). The audit now classifies 467
      constants; 136 newly classified fixed ceilings remain deferred policy
      work, not newly configurable limits.
- [ ] Run focused Core, example, integration, publish-helper, and docs tests
      (client 66/66 before the latest focused process regression, example
      type-checks, and publish 18/18 pass; opt-in live
      SQLite/VM restart/cron and local SQLite/VFS checks pass; all native
      sidecar test targets compile; Astra's kernel API 33/33 and focused native
      4/4 checks pass; Flue's hosted provider 5/5 and embedded native test pass;
      latest sidecar package 4/4, configure-policy 2/2, and operator-unlink
      1/1 tests pass; Rust client process 8/8, actor process 5/5, TypeScript
      collision 6/6, and TypeScript mount/RPC 7/7 tests pass; remaining runtime
      and docs gates remain. The new package-mount service regressions (2/2),
      operator unlink (1/1), projection (11/11), typed limits (8/8), and Rust
      client limit serialization (1/1) pass). The full TypeScript Core unit
      suite passes (20 files, 103 tests), as do the latest Rust client process
      units (11/11), actor action-adapter units (8/8), and real-VM Node timeout
      regression (including a bounded VM teardown). The native-sidecar-core
      snapshot tests pass (4/4), along with the new kernel argv and timer-scope
      regressions (1/1 each). The latest hosted actor library suite passes
      (64 passed, 1 opt-in live test ignored), including the config classifier
      and typed-error regressions. Astra's latest embedded Rust client suite
      passes (77/77) with its contract-feature check and real-VM rejection/late
      wait regression; TypeScript Core passes 114/114 with type checks. Final
      runtime/docs gates remain. The full `pnpm test` run exposed a resolver
      test that assumed no platform package existed above the workspace; its
      isolated rerun now passes with deterministic package-resolution fixtures
      and rejects mismatched or missing native binaries (7/7, including an
      exact-version repair command). The next full
      run exposed a stale `software/everything` test expectation for a removed
      Codex package; its focused test now passes (2/2). A further full run
      reached Core and exposed two pre-existing nested `npm test` hangs and
      repeated VM SQLite-close errors. Both owned and internal VM disposal now
      close capability admission after process drain, then close SQLite before
      retiring blocking-job admission; a real local-DB
      regression covers both paths, and an embedded smoke test no longer logs
      the close error. The nested npm tests still time out in isolation with a
      freshly rebuilt sidecar. Full-suite validation remains open.
      The sidecar-resolved config comparison and session-owned wire dispatch
      tests pass (2/2); the hosted actor suite passes 68/68 with one opt-in test
      ignored. Astra's TypeScript protocol/transport comparison tests pass
      (14/14), and all 103 workspace TypeScript type-check tasks pass after
      this protocol change. The Rust Core default-feature build also passes
      without the actor-only comparison helper.
      Final workspace gates after Astra's fixes pass: `cargo check --workspace`,
      `pnpm check-types` (103/103), `pnpm build` (50/50),
      Rust client tests (78/78), `cargo fmt --all -- --check`, and fixed-version
      verification. The later inspector/process-input pass also passes 33/33
      inspector tests, 71 actor library tests (one opt-in live test ignored),
      3/3 native shell-argument tests, `cargo check --workspace`, workspace
      `pnpm check-types` (103/103), `pnpm build` (50/50), and formatting.
      After protocol version 9 and the session acquisition review, the Rust
      workspace check, protocol tests (8/8), native acquisition tests (4/4),
      resolver/cache tests (19/19), TypeScript runtime-core protocol tests
      (25/25), Core cross-language fixtures (7/7), workspace `pnpm build`
      (50/50), workspace `pnpm check-types` (103/103), and formatting pass.
- [ ] Run `pnpm --dir website build` when the website checkout is available.
- [ ] Run `just docs-check-links` for changed routes and navigation.
- [x] Have an Astra reviewer inspect the complete diff, fix issues, and rerun
      affected checks. A second Astra pass on the late process snapshot and
      timer-wheel fixes added focused regression coverage. A further lifecycle
      pass removed fabricated process exits in both embedded clients, fixed
      normalized mount ordering, and added focused and real-VM regressions.
      The latest Astra pass found and fixed package rejection type erasure,
      missing TypeScript wire adapters, and unbounded sidecar acquisition
      waiters; it also added protocol version 9 and timeout regressions.

### Open implementation and verification work

- Move VM config normalization, default materialization, and live/restart
  classification out of the actor into one sidecar-owned contract. The actor
  currently keeps optional runtime fields. For running VMs it now compares
  resolved creation policy for whitelisted runtime fields through a
  session-scoped sidecar request, so an explicit override equal to the
  effective default does not force a restart. Software, hosted filesystem
  policy, and future unmapped fields remain replacement-triggering.
  When no VM is serving, it conservatively compares desired fields. The shared
  VM config crate now bounds and canonicalizes Node builtin and loopback-port
  overrides for the actor, embedded client, and sidecar; it rejects oversized
  duplicate-heavy input before deduplication. The Rust embedded client sends
  only explicit permission overrides at VM creation and leaves permissions
  unset during boot and dynamic-mount configuration, matching TypeScript and
  preserving the sidecar-resolved defaults; its copied network allowlist has
  been removed. Actor restart classification now compares the complete desired
  document except preview policy, so newly added fields are fail-safe until
  deliberately classified. Ownership of the classification still needs to move
  to the sidecar contract. Completing normalization ownership and an
  independent no-running-VM comparison remains open.
- Preserve live software across dynamic mount changes. The sidecar now tracks
  identities added by `LinkPackage` and composes them into later `ConfigureVm`
  projections until an explicit `UnlinkPackage`; the TypeScript client no
  longer mirrors them. Pinned packages retain their original projection roots;
  conflicting IDs, package/command names, and normalized mount paths are
  rejected before changing mounts. Regressions exercise link, mount add/remove,
  custom-root retention, and unlink. Client-originated mount and software
  mutations, including public `link_software`, share an async lock.
  `ConfigureVm` still replaces other boot fields and all mounted leaves, so a
  dedicated mounts-only operation and
  cross-client reconfiguration semantics remain follow-up work.
- Make all VM configuration and package unlink mutations transactional or
  define an explicit recoverable failure state. Replacement mounts are
  preflighted for valid paths, registered plugins, duplicate paths, and
  parseable plugin config. The mount phase now detaches old backends without
  closing them, removes any replacement leaves on failure, and restores the
  exact old backends. A failed package-link mount group also removes earlier
  leaves before publishing package metadata. Rollback removes only newly
  materialized empty mountpoint directories, before detaching their temporary
  parents, preserving pre-existing underlying directories. Detached backends
  are closed if abandoned, and attempted mount mutations invalidate usage
  caches even on failure. Focused regressions cover malformed input, later
  plugin-open and package-leaf failures, and directory preservation. Mount
  setup and teardown sort by normalized path depth, including dot segments.
  Remaining: roll back changes after the mount phase (command discovery/driver
  registration, package runtime-state refresh, and bridge policy publication),
  make package unlink's multi-mount and
  empty-directory cleanup reversible, and define a recoverable state if any
  rollback operation itself fails. The operator path preserves nonempty guest
  directories, but that alone does not make unlink atomic.
- [x] Enforce an aggregate per-VM package mount limit, including runtime links
  and `provides.files` leaves. `limits.agentosPackages.maxMounts` now defaults to
  4,096, can be raised, and is checked before mutating the live projection. A
  near-limit warning and structured resource-limit rejection name the setting.
  Capacity is checked while building the combined projection. Live accounting
  uses installed mount paths, not a reread of mutable package sources; unlink
  releases those paths without requiring spare capacity. Focused tests cover
  boot, `provides.files`, dynamic link, source changes, unlink, and overrides.
- [x] Expose matching `tls` and `execution` limit groups in Rust Core,
  TypeScript Core, and the generated actor contract. Serialization preserves
  omitted sidecar defaults; focused tests cover wire names, invalid overrides,
  and optional public types without duplicating runtime policy.
- Move package source resolution into the sidecar. The version 10 protocol has
  a session-scoped `AcquirePackage` operation for config resolution and
  preloading, a VM-scoped `InstallPackage` operation that verifies and pins
  bytes before projection, and a session-scoped cache-stats operation. The
  sidecar checks ownership before I/O. Rust Core now forwards a closed URL or
  trusted-path source; the hosted actor accepts URLs only. The actor's required
  installs, config resolution, advisory preload, and metrics all use the same
  child-sidecar cache. Child cache settings are passed at process startup, and
  the actor retains a pre-VM session until worker shutdown. Rust clients
  preserve package-acquisition wire codes and rejection
  metadata (including errno, retryability, and configuration paths) in a
  dedicated typed error instead of misclassifying them as kernel failures.
  Limits and timeouts retain their existing typed categories. The actor maps
  invalid sources, formats, and digests to `invalid_input`; operator cache
  configuration failures are not misreported as caller validation errors.
  Public actor errors retain the package wire code and metadata under
  `meta.package`.
  The wire still lacks structured digest-mismatch fields, so it cannot safely
  reconstruct every original local `ClientError` variant without parsing text.
  - [x] Add a session-scoped sidecar acquisition operation so desired-only
        config and advisory preload can resolve packages without a VM.
  - [x] Move one bounded resolver/cache and its URL validation, digest checks,
        single-flight admission, and eviction policy to the sidecar process.
        The actor retains URL-only input policy and coordinator orchestration.
  - [x] Make VM installation accept a closed URL/trusted-path source, derive
        identity from verified bytes in the sidecar, and retain an artifact pin
        until all package mounts are safely removed. Keep legacy trusted
        directory linking distinct from verified `.aospkg` installation.
  - [x] Start and configure the shared sidecar before worker preload; retain
        its acquisition session while warm cache state is needed even with no
        live VM. Remove actor-parent cache initialization to avoid two caches
        or exclusive directory-lock contention.
        The child is explicitly reaped before removing the worker's temporary
        cache directory. The worker retains the sidecar handle before opening
        the package session, so cancellation during session initialization
        cannot hide a live child from shutdown cleanup.
  - [ ] Finish the cancellation and cross-client parity audit. A dropped actor
        waiter does not instantly cancel an already-sent sidecar request;
        per-request remaining-time and operator timeouts bound it. Verify
        shared-flight behavior for optional and required waiters, URL/path
        digest identity, source mutation, pin/eviction lifecycle, and the
        TypeScript Core install/list API. Local paths without an expected digest
        now invalidate their source alias before every read, so replaced or
        deleted bytes cannot be hidden while overlapping reads still
        single-flight. Rust and TypeScript Core now admit only one local mount
        or software mutation at a time and reject overlap with `invalid_state`;
        neither client creates an unbounded waiter queue. Rust's
        package-session/cache controls are hidden behind actor/sidecar
        integration features rather than exposed as an unmatched Core API, and
        both public Core installed-package DTOs use an explicit `sizeBytes`
        field (`size_bytes` in Rust). The binary-level pre-VM child-cache test
        passes, and the TypeScript Core now forwards install/uninstall and
        returns isolated metadata snapshots. Focused Rust and TypeScript
        concurrency regressions pass.
- [x] Make creation cancellation owned end to end. Explicit Core initialization
      errors dispose a VM created earlier in the sequence. Rust Core runs
      creation in an owned task; dropping the caller transfers the task to a
      cleanup waiter that shuts down any late VM result. The actor likewise
      keeps its creation task alive after its own deadline. A focused regression
      covers detached Core task ownership; the actor library suite covers the
      surrounding lifecycle state machine.
- The ignored Rivet SQLite driver probe passed against an already healthy,
  isolated local engine: callback values, transaction rollback, and generation
  claims all used the real RivetKit driver. The ignored standalone-actor test
  passed VM restart with a new durable generation and an unchanged applied
  config revision. Cold `rivetkit::test::setup` remains unreliable in this
  workspace: it exits after 60 seconds with `wait for registry envoy startup:
  channel closed`, including with isolated storage and the cached engine binary.
- The ignored standalone-actor test also passed durable cron schedule/list
  across VM restart, cancellation, and a real scheduled invocation observed as
  `cron.fired`. Focused tests cover cancelled, replaced, and forged private
  payloads. These opt-in live tests require a prestarted local Rivet engine and
  remain outside the default CI suite. The restart/cron probe now isolates its
  sidecar binary and cache, bounds the whole probe, cancels its jobs on failure,
  and drains its process group; the revised probe passed against the isolated
  local engine. Actor metadata remains in that disposable engine's test storage.
- Bound the entire `process.run` lifecycle and confirm guest termination.
  Rust Core now budgets launch, stdin, and execution together; actor ready-VM
  lookup is also bounded. SIGKILL acknowledgement and matching VM/process exit
  are observed concurrently within a 30-second cleanup window. Only confirmed
  exit returns `timeout`; otherwise the actor exposes typed `termination_failed`.
  TypeScript timeout cleanup uses an internal confirmed-exit channel. Ordinary
  embedded TypeScript and Rust waits now report typed launch/observation
  failures instead of synthetic exit codes; late real exits remain observable,
  and Rust fast exits are retained for late waiters. Cancellation retains
  the pre-launch event subscription and starts bounded, logged cleanup; known
  admission rejections do not create detached confirmation work. Unit and
  real-proxy mock regressions cover confirmation identity and failure paths.
  The real-VM Node regression observes a running kernel PID and confirms its
  exit. It also uncovered that process snapshots dropped runtime arguments and
  a process-global timer-wheel worker was charged to the first VM; both are
  fixed, and teardown no longer hits the five-second reconciliation deadline.
  Still open: sidecar-owned cancellation/tombstones for remotely accepted
  Execute requests that register after cleanup expires, cleanup after kernel
  allocation but before registration, and bounded admission for detached cleanup
  tasks. A local waiter abort cannot revoke a remote launch. Verify slow
  launch/prewarm, blocked stdin, capture overflow, actor cancellation, and
  termination failure across JavaScript, Python, and WASM, including the
  six-minute framework deadline. TypeScript's one-shot exit callback does not
  automatically resubscribe after an observation failure, and ambiguous live
  entries remain retained until a real exit or VM disposal. An observation
  failure is not proof of guest termination.
- The bounded action adapter supports integer and binary scalar tags,
  explicit undefined properties, and escaped literal arrays. Non-DTO values
  such as JavaScript `Set` remain unsupported; use JSON arrays/objects instead.
- SQLite close and the subsequent resource drain now share
  `limits.reactor.shutdownDeadlineMs`; process termination and filesystem
  flushing precede this budget. A canceled close returns typed `timeout` and
  marks the generation's close as unconfirmed. Local counts reaching zero
  cannot prove a host callback finished, so this bounded quarantine is not
  automatically reaped and requires a sidecar worker restart. Remaining:
  observe eventual close completion safely before reclaiming these generations,
  and bound the preceding synchronous flush/teardown phases. Rust Core and both
  TypeScript proxies now preserve a rejected DisposeVm result through secondary
  cleanup. The hosted actor's fixed ten-second outer wait can still cancel Core
  cleanup before a raised sidecar deadline; the runtime test proxy's one-second
  wait now rejects explicitly but also cannot confirm remote completion. Unify
  these waits with sidecar policy and make canceled cleanup completion observable.
  Sidecar-originated timeout and resource-limit details now cross the actor
  boundary as structured `metadata.limit` fields; actor-only deadlines have no
  sidecar limit metadata.
  The older runtime test proxy also retains synthetic exit fallbacks outside
  disposal; the Core process-observation fixes have not been ported there.
- Run the coordinator and persistent package cache under a representative
  multi-process deployment to tune disk reserve, capacity, and preload metrics.
  The pinned RivetKit `2.3.12-rc.3` actor-registry `/metrics` handler renders
  the process registry before actor dispatch and does not call its metrics
  token gate; unlike the serverless route, it is not safe to expose without a
  deployment-level authenticated proxy or private network boundary. Verify
  that boundary in the representative deployment before enabling scraping.
  The hosted actor library suite now passes 66/66 with one opt-in live SQLite
  test ignored after the disposal error metadata changes.
- Investigate the two nested `npm test` Core integration cases. Both now time
  out while awaiting the guest process even in an isolated run with a newly
  rebuilt sidecar; this is separate from the repaired SQLite teardown order. A
  live probe printed npm's script header but not the child command's output;
  the guest tree still showed only the parent shell and npm's Node process.
- Re-run the website build and link crawl when the website checkout and Docker
  daemon are available. This workspace cannot run those gates.

## Completion criteria

This step is complete only when the generated TypeScript surface matches the
Rust registry, the hosted actor has no local SQLite persistence path, all
public terminology and replay shapes are consistent, config mutations share
one tested engine, docs name concrete limits and behaviors, applicable
inspector tabs use the public actor contract, and all required validation plus
the independent Astra review pass.
