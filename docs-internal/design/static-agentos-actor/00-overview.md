# Static agentOS Actor Refactor

**Status:** Implemented locally; production deployment smoke remains

## Outcome

Replace the configurable TypeScript hosted actor with one fixed Rust actor named
`agentOS`. The actor is the Rivet lifecycle and transport adapter for the sandbox
surface already implemented by agentOS Core. It must not become a second runtime
implementation.

This project also removes agents, sessions, ACP, and their storage and transport
paths from the entire agentOS repository. The TypeScript Core package remains the
embedded SDK and keeps host bindings. Host bindings are deliberately absent from
the hosted `agentOS` actor because actor creation and action calls cannot safely
carry callbacks, closures, or host objects.

The work is split into the numbered specifications in this directory. Each file
is intended to map to one reviewable PR and one `jj` revision unless implementation
reveals a hard dependency that requires a smaller split.

## Locked decisions

- The hosted actor's registered name is `agentOS`.
- There is one prebuilt actor implementation. Consumers cannot provide actor
  source, callbacks, lifecycle hooks, a custom VFS object, or arbitrary actor
  options.
- The hosted actor exposes only sandbox functionality. Agents, sessions, ACP,
  adapters, session history, and their events are removed globally.
- `@rivet-dev/agentos-core` remains available for embedded use and retains host
  bindings.
- The hosted actor does not expose host bindings or host mounts.
- Durable mutable configuration may be supplied when the actor is created and
  replaced later with `config.set`. A replacement normalizes the entire input;
  omitted properties reset to Core-owned defaults.
- Graceful runtime shutdown and process draining are deferred follow-up work.
  This refactor promises only the existing bounded Core replacement behavior.
- Configuration actions use dotted RivetKit action names. The generated
  TypeScript client presents those actions as nested objects.
- The TypeScript hosted actor is deleted before the Rust replacement is built.
  It is not incrementally ported.
- Actor Runtime Socket support is removed before the Rust actor is introduced.
- During the proof of concept, actor and Core persistence use small local SQLite
  adapters. The adapters preserve the eventual Rivet SQLite boundary and are not
  allowed to leak local-database details into Core APIs.
- The executable may expose two entry points: the normal Rivet actor process and
  an internal native sidecar process. Neither entry point carries ACP.
- Registry software is not published to or resolved from npm. `.aospkg`
  artifacts are published at stable HTTPS URLs backed initially by
  S3-compatible object storage. The hosted actor installs URL sources only;
  trusted embedded Core callers may also install from local paths.
- The package cache has a process-local content-addressed layer. A separate
  central actor stores an approximate, low-frequency hot-package set for startup
  preloading. The central actor is advisory and is never on an execution request's
  correctness path.
- The current configurable guest filesystem remains supported through safe,
  serializable descriptors resolved by a fixed filesystem registry. Host paths,
  host mounts, callback mounts, unknown filesystem implementations, and
  JavaScript VFS objects are forbidden in the hosted actor.
- The RivetKit typed-action-count change is not part of this project.
- Preview tokens remain TTL-bound actor SQLite records served through the
  actor's ordinary request handler.
- Hosted cron uses RivetKit's durable schedule API and invokes the actor's process
  execution action. It does not add an agentOS scheduler database.
- No API, wire, storage, package, or deployment compatibility is required.

## Architecture and ownership

### 1. Static Rust actor

The actor owns only responsibilities that require Rivet actor context:

- Rivet actor startup, shutdown, actions, events, and keep-awake integration.
- Loading creation input and durable desired configuration.
- Persisting configuration revisions and coordinating runtime reconciliation.
- Translating serializable action DTOs to one-for-one Core client calls.
- Mapping Core errors into stable typed actor errors without hiding their cause.
- Connecting preview-token request paths to the VM networking API.
- Routing URL-based package installation and consulting process-local package
  services.
- Emitting bounded process, terminal, runtime, and schedule events.

It must not implement filesystem semantics, process policy, language execution,
networking policy, permission decisions, guest defaults, or package projection.
Those remain in Core, the sidecar, the kernel, and the VFS.

### 2. Rust Core client

The Rust client is the actor's direct API. It supplies typed requests and
responses for VM lifecycle, files, processes, terminals, language execution,
networking, packages, and scheduling. Actor actions should normally contain
validation of the wire DTO, one Core call, and transport-safe encoding of the
result.

If actor implementation requires behavior missing from the Rust client, add it
to the shared Core/sidecar contract first. Do not implement the behavior only in
the actor.

### 3. TypeScript Core

`@rivet-dev/agentos-core` remains the embedded TypeScript API. It keeps host
bindings because an embedding application can provide in-process callbacks.
It loses all agent, session, ACP, Actor Runtime Socket, and actor-specific code.
Its sandbox behavior must remain in lockstep with the Rust client and sidecar.

### 4. Native sidecar, runtime, kernel, and VFS

These components remain the trusted enforcement point. They own VM defaults,
permissions, resource limits, the process and socket tables, filesystem
semantics, language runtimes, package projection, and the `/opt/agentos` view.
Removing ACP must leave a generic sidecar that is usable by both the embedded
TypeScript Core and the static Rust actor.

### 5. SQLite adapters

The proof of concept uses local SQLite behind explicit adapter interfaces. The
physical database may be shared, but schema ownership is independent:

- Filesystem storage owns `agentos_fs_*` and `agentos_fs_schema_version`.
- Core durable state owns `agentos_core_*` and
  `agentos_core_schema_version`.
- The Rust actor owns `agentos_actor_*` and
  `agentos_actor_schema_version`.

Every table is `STRICT`. Each owner migrates only its own namespace and advances
its own version in the same transaction. There is no adoption or compatibility
path for legacy `agentos_vfs_*`, `agentos_session*`, or `agent_os_*` tables.

### 6. Package source and package cache

Core resolves a closed package source enum: remote URL or trusted embedded local
path. Both paths produce verified immutable `.aospkg` content in the process
cache, which is memory-mapped and projected by VFS under `/opt/agentos`. The
hosted actor exposes only the URL variant and can never construct a local-path
source. Cache identity is the content digest, not the URL.

### 7. Preload coordinator actor

The coordinator stores an approximate bounded hot set of exact URL, digest, and
size identities.
Each agentOS process reads it once during startup and warms the process-local
cache before becoming ready, subject to a strict timeout. Usage messages are
coalesced in each process and flushed infrequently. Missed, duplicated, stale, or
reordered messages affect only cold-start performance.

### 8. Generated TypeScript contract

The Rust action and event contract is the source of truth. An agentOS-specific
prototype in `packages/agentos-bindgen` exports schema and generates TypeScript
without modifying RivetKit. Possible later generalization is not part of this
refactor. The generated client adds ergonomic binary conversions and event
subscriptions but no policy, defaults, persistence, or state machine.

## Internal component API surfaces

These are implementation boundaries, not public actor actions. Names are
illustrative Rust traits or internal actor actions; the important constraint is
that dependencies point inward toward Core and outward toward infrastructure
through narrow adapters.

### Core runtime adapter

```text
start(config, event_sink) -> RuntimeHandle
status(handle) -> CoreRuntimeStatus
classify_config_change(current, candidate) -> ReconcilePlan
apply_live(handle, plan) -> AppliedChange
stop(handle, deadline) -> ShutdownResult
```

All filesystem, process, terminal, context, language, network, software, and cron
methods are supplied by the shared Rust Core client using the public operation
names. The actor adapter adds no parallel versions of them.

### Actor state store

```text
initialize(create_input) -> ConfigSnapshot
get_config() -> ConfigSnapshot
prepare_replacement(expected_revision, input) -> NormalizedCandidateConfig
commit_desired(candidate) -> ConfigSnapshot
mark_applied(revision, runtime_generation) -> ConfigSnapshot
mark_failed(revision, typed_issue) -> ConfigSnapshot
```

This interface is backed temporarily by local SQLite and later by Rivet SQLite.
It owns `agentos_actor_*` only.

### Core and filesystem database adapters

```text
transaction(callback) -> result
execute(statement, parameters) -> ExecuteResult
query(statement, parameters) -> bounded rows
close() -> result
```

The actual trait should prevent raw cross-namespace migration access where
practical. It must not expose a local file path as part of the durable API.

### Hosted filesystem registry

```text
register(id, config_schema, capabilities, factory)
validate(descriptor) -> ValidatedFilesystemDescriptor
open(validated_descriptor, actor_storage) -> CoreFilesystem
```

The static binary constructs this registry from a fixed whitelist. Actor input
can select and configure a registered filesystem but cannot add a factory or
identify a host path.

### Package source adapter

```text
resolve(Url { url, expected_digest? }, limits) -> VerifiedPackage
resolve(Path { path, expected_digest? }, limits) -> VerifiedPackage
project(packages, runtime_generation) -> InstalledPackageSet
```

Only embedded trusted Core callers can create `Path`. The actor contract
contains only `Url`. Both variants publish verified content into the same
digest-keyed cache and projection path.

### Process-local package cache

```text
get(digest) -> CacheHit | CacheMiss
get_or_fetch(remote_source, fetcher) -> CachedArtifact
pin(digest, runtime_generation) -> Pin
release(pin) -> void
warm(resolved_remote_sources, deadline) -> WarmReport
record_access(digest) -> void
stats() -> CacheStats
```

The cache is immutable by digest, byte-bounded, LRU-evicted, and single-flight
per digest.

### Preload coordinator actor

| Internal action | Purpose |
| --- | --- |
| `preload.getPlan` | Read one bounded exact-artifact hot set at process startup |
| `preload.recordUsage` | Submit one coalesced process observation window |
| `preload.replaceBaseline` | Replace the operator-managed seed set, if action-managed |
| `preload.status` | Inspect bounded coordinator health and plan metadata |

No public agentOS action forwards these calls.

### Preview token store

```text
create(port, expires_at) -> PreviewToken
resolve(token, now) -> port | expired
expire(token) -> void
```

Tokens are actor SQLite rows used by the actor's ordinary `on_fetch` route. They
survive sleep until their TTL and never expose arbitrary host routing.

### RivetKit scheduling adapter

```text
cron_set(name, expression, "process.exec", args) -> void
cron_list() -> bounded jobs
cron_delete(name) -> bool
```

The actor uses RivetKit's existing durable cron storage and wakeup machinery. A
scheduled private action invokes the same Core process execution path as a direct
`process.exec` action.

### Contract generator

```text
export_schema(rust_actions, rust_events, rust_errors) -> ContractSchema
generate_typescript(contract_schema) -> package sources
verify_contract(contract_schema, registered_handlers) -> result
```

This contract generator lives in the agentOS workspace package. The generated
client uses the vanilla RivetKit actor proxy and recreates nested objects from
dotted names.

## Actor lifecycle

1. Rivet creates or wakes the `agentOS` actor with optional `config` creation
   input.
2. The actor opens its SQLite adapter and migrates only `agentos_actor_*`.
3. On first creation, RivetKit passes creation input to `create_state`; validation
   failure aborts creation. Once state exists, later `getOrCreate` input is not an
   update and `config.set` is the only mutation path.
4. The process reads the preload coordinator once and begins bounded package
   warming. Readiness waits for warming only until the configured startup
   deadline. A timeout is reported but does not corrupt desired state.
5. Core resolves desired package URLs, verifies and caches immutable content,
   constructs the VM, and projects `/opt/agentos` from memory-mapped bytes.
6. Health becomes ready only after the desired configuration has been applied
   and required startup packages are available. It reports degraded or failed
   state with typed causes rather than returning a false success.
7. Actions call Core. Live events are forwarded through bounded actor event
   paths, while replayable process and terminal output comes from bounded Core
   buffers.
8. A complete normalized config replacement is durably committed with a
   monotonic revision. The actor applies it live when Core supports that change;
   otherwise it records that runtime replacement is required.

## Durable configuration contract

The public shape below is conceptual. Exact field types should reuse generated
Core protocol DTOs rather than copy handwritten actor types.

```ts
interface AgentOsActorCreateInput {
  config?: AgentOsActorConfigInput;
}

interface AgentOsActorConfigInput {
  user?: VmUserConfig;
  environment?: Record<string, string>;
  software?: RemotePackageSource[];
  allowedNodeBuiltins?: string[];
  highResolutionTime?: boolean;
  filesystem?: {
    root?: HostedRootFilesystemDescriptor;
    mounts?: HostedMountDescriptor[];
  };
  permissions?: PermissionConfig;
  limits?: AgentOsLimits;
  preview?: PreviewPolicy;
}

interface AgentOsConfigSnapshot {
  revision: number;
  desired: CompleteAgentOsActorConfig;
  appliedRevision: number | null;
  state: "applying" | "ready" | "restart_required" | "failed";
  issues: ConfigIssue[];
}
```

Every property has a Core-owned default. Creation and `config.set` pass the input
through the same Core normalizer, and the actor persists the resulting complete
document. The actor does not maintain copied default constants.

Mutation is full replacement:

```ts
interface SetConfigInput {
  expectedRevision?: number;
  config: AgentOsActorConfigInput;
}
```

Omission means reset to the default, including for arrays and maps. A caller that
wants to preserve existing values reads the complete config with `config.get`,
modifies it, and writes it back with `expectedRevision`. Explicit empty lists,
empty maps, `false`, and valid zero values remain distinct from omission. There
is no merge patch, operation list, or array-specific behavior.

`expectedRevision` provides optional optimistic concurrency. Validation and
resource preparation happen before commit where possible. If reconciliation
fails after a commit, the desired revision stays durable and the snapshot exposes
the failure; the actor must not claim that the previous revision is current.

Operator-only settings are process configuration, not mutable actor state. These
include SQLite implementation details, cache storage and byte limits, package
download egress policy, preload coordinator location, preload deadlines, actor
transport limits, sidecar executable selection, and logging.

## Core-only public MVP surface

The hosted action list below is deliberately smaller than embedded Core. These
operations remain available to trusted Core callers and are never exposed as
actor actions:

| Core method or config | Why it is Core-only |
| --- | --- |
| `runtime.create` / `runtime.start` | RivetKit owns actor creation |
| `runtime.configure` | The actor exposes durable whole-document `config.set` |
| `runtime.dispose` | RivetKit owns actor shutdown |
| `filesystem.mount` / `filesystem.unmount` | The actor changes its whitelisted mount list through `config.set` |
| `process.kill` | Actor clients use `process.signal` with `SIGKILL` |
| `network.httpRequest` | Low-level primitive beneath actor-safe VM-port fetch actions |
| trusted `software.install(Path)` | Hosted software sources are URL-only |
| host binding and custom filesystem configuration | These values are not serializable or safe actor input |

Core and the actor both expose runtime status/restart, filesystem observation and
I/O, processes, terminals, contexts, language execution, VM-port fetches,
software install/uninstall/list, and process-command cron. Cron has the same
public intent but different infrastructure: embedded Core uses its in-process
scheduler and the actor uses RivetKit cron.

## Complete actor API surface

All names below are RivetKit action names. Dots are part of the registered name;
the TypeScript client projects them as nested properties. Request and result DTOs
come from the shared generated protocol unless explicitly actor-owned.

### Configuration and runtime

| Action | Input | Result | Ownership |
| --- | --- | --- | --- |
| `config.get` | none | `AgentOsConfigSnapshot` | Actor state |
| `config.set` | `SetConfigInput` | `AgentOsConfigSnapshot` | Actor reconciliation |
| `runtime.status` | none | `RuntimeStatus` | Actor plus Core observation |
| `runtime.restart` | `RestartRuntimeInput?` | `RuntimeStatus` | Actor lifecycle |

`runtime.status` replaces the old flat `health` action and must not boot a
sleeping VM merely to observe it. Platform health checks may call the same
implementation. Readiness distinguishes package warming, booting, ready,
degraded, stopping, and failed states.

### Filesystem

| Action | Input | Result |
| --- | --- | --- |
| `filesystem.readFile` | `{ path, maxBytes? }` | byte string |
| `filesystem.writeFile` | `{ path, content }` | none |
| `filesystem.readFiles` | `{ paths, maxBytes? }` | per-path content or error |
| `filesystem.writeFiles` | `{ entries }` | per-path success or error |
| `filesystem.stat` | `{ path }` | `VirtualStat` |
| `filesystem.mkdir` | `{ path, recursive? }` | none |
| `filesystem.readdir` | `{ path }` | names |
| `filesystem.readdirEntries` | `{ path }` | typed entries |
| `filesystem.readdirRecursive` | `{ path, maxDepth?, exclude? }` | typed recursive entries |
| `filesystem.exists` | `{ path }` | boolean |
| `filesystem.move` | `{ from, to }` | none |
| `filesystem.remove` | `{ path, recursive? }` | none |
| `filesystem.export` | `{ maxBytes? }` | root snapshot |
| `filesystem.listMounts` | none | live Core mount enumeration |

Binary fields use a transport-safe byte representation selected by the generated
contract. Bulk and export operations have explicit item and byte limits. If full
filesystem export cannot fit within an action response limit, the contract must
become a bounded chunk stream before release.

### Processes

| Action | Input | Result |
| --- | --- | --- |
| `process.exec` | `{ command, options? }` | captured result |
| `process.execFile` | `{ command, args?, options? }` | captured result |
| `process.spawn` | `{ command, args?, options? }` | generation-scoped process id |
| `process.get` | process id | process metadata |
| `process.list` | none | tracked process metadata |
| `process.tree` | none | generation plus process forest |
| `process.wait` | process id | exit result |
| `process.signal` | `{ process, signal }` | none |
| `process.writeStdin` | `{ process, data }` | none |
| `process.closeStdin` | process id | none |
| `process.resizePty` | `{ process, cols, rows }` | none |
| `process.readOutput` | `{ process, after?, maxEvents?, maxBytes? }` | sequenced replay |

### Terminals

| Action | Input | Result |
| --- | --- | --- |
| `terminal.open` | `{ options? }` | generation-scoped terminal id |
| `terminal.list` | none | terminal metadata |
| `terminal.snapshot` | `{ terminal, after?, maxBytes? }` | ordered raw PTY replay |
| `terminal.write` | `{ terminal, data }` | none |
| `terminal.resize` | `{ terminal, cols, rows }` | none |
| `terminal.wait` | terminal id | exit result |
| `terminal.close` | terminal id | none |

### Execution contexts

Context handles are `{ generation, contextId }`; a runtime restart invalidates
old handles.

| Action | Input | Result |
| --- | --- | --- |
| `contexts.create` | `{ contextId }` | normalized context descriptor |
| `contexts.get` | context handle | context descriptor |
| `contexts.list` | none | bounded context descriptors |
| `contexts.reset` | context handle | refreshed context descriptor |
| `contexts.delete` | context handle | none |

### JavaScript and npm execution

| Action | Core operation |
| --- | --- |
| `javascript.execute` | execute source |
| `javascript.evaluate` | evaluate an expression |
| `javascript.executeFile` | execute a file |
| `javascript.spawn` | spawn source execution |
| `javascript.spawnFile` | spawn file execution |
| `javascript.npm.install` | install guest project dependencies |
| `javascript.npm.runScript` | run a package script |
| `javascript.npm.runPackage` | run a package executable |

The npm actions in this group operate inside the guest project. They are
different from URL-installed `.aospkg` registry software projected under
`/opt/agentos`. For `javascript.npm.install`, an empty package list means
install the guest project; a non-empty list installs those explicit packages.

### TypeScript

| Action | Core operation |
| --- | --- |
| `typescript.execute` | transpile and execute source |
| `typescript.evaluate` | evaluate an expression |
| `typescript.executeFile` | execute a file |
| `typescript.spawn` | spawn source execution |
| `typescript.spawnFile` | spawn file execution |
| `typescript.check` | check supplied source |
| `typescript.checkProject` | check a guest project |

### Python

| Action | Core operation |
| --- | --- |
| `python.execute` | execute source |
| `python.evaluate` | evaluate an expression |
| `python.executeFile` | execute a file |
| `python.executeModule` | execute a module |
| `python.spawn` | spawn source execution |
| `python.spawnFile` | spawn file execution |
| `python.spawnModule` | spawn module execution |
| `python.install` | install guest Python dependencies |

### Networking and previews

| Action | Core operation |
| --- | --- |
| `network.fetch` | buffered request to a VM port |
| `network.fetchStream.start` | start a bounded Core/sidecar streaming response |
| `network.fetchStream.read` | read the next chunk, capped at 128 KiB |
| `network.fetchStream.cancel` | release a stream |
| `network.preview.create` | create a TTL-bound actor gateway token for a VM port |
| `network.preview.expire` | idempotently expire a preview token |

Stream handles include the runtime generation and absolute expiration. The
sidecar owns the bounded stream registry and releases streams on completion,
error, idle timeout, VM shutdown, or explicit cancellation; the actor does not
duplicate that registry.

### agentOS registry software

| Action | Input | Result |
| --- | --- | --- |
| `software.list` | none | live `/opt/agentos` package view |
| `software.install` | remote URL, optional expected digest, expected config revision | installed package and config snapshot |
| `software.uninstall` | exact installed package id, expected config revision | config snapshot |

Creation and whole-document `config.set` can also specify the desired URL source
list. `software.install` and `software.uninstall` are explicit domain mutations
of that same config revision, not an independent package database. No hosted
software action accepts a local path, and actor decoding cannot construct Core's
trusted `Path` source variant.

### Scheduled commands

| Action | Core operation |
| --- | --- |
| `cron.schedule` | expression, timezone, process-spawn descriptor, config revision | persisted command schedule |
| `cron.list` | none | bounded command schedules |
| `cron.cancel` | schedule name | whether a schedule was removed |

The only supported scheduled target is a process execution descriptor. Session
prompt scheduling is removed. Each persisted schedule records the config
revision it was validated against and emits an explicit failure if it can no
longer be executed.

### Events

| Event | Payload |
| --- | --- |
| `runtime.booted` | runtime id, applied config revision, timestamp |
| `runtime.shutdown` | reason, exit status, timestamp |
| `runtime.limitWarning` | typed limit, current value, maximum, how to raise it |
| `process.output` | process id, sequence, stdout/stderr channel, bytes |
| `process.exit` | process id, exit status and signal |
| `terminal.data` | terminal id, sequence, bytes |
| `terminal.stderr` | terminal id, diagnostic bytes |
| `terminal.exit` | terminal id, exit status |
| `cron.fired` | schedule name, process id or typed launch error, timestamp |

Events are live notifications, not the only recovery mechanism. Process and
terminal consumers recover gaps with `process.readOutput` and
`terminal.snapshot`. Every event queue and replay buffer has a default bound,
warning threshold, and typed overflow behavior.

### Deliberately absent

- `agents.*`
- `sessions.*`
- ACP transport, capabilities, prompts, permissions, or history
- actor-configured host bindings
- actor-configured host mounts or arbitrary host paths
- lifecycle callbacks such as `onVmStart` and `onVmStop`
- caller-supplied actor implementation or `resolveOptions` callback
- legacy flat action aliases
- direct database, sidecar, scheduler-driver, or cache implementation options

## Error contract

Actor failures use RivetKit's universal serializable error envelope:

```ts
interface AgentOsActorError {
  group: string;
  code: string;
  message: string;
  metadata?: Record<string, unknown>;
}
```

The generated TypeScript contract uses `RivetError` rather than product-specific
error classes. Limit errors name the configured limit and how to raise it. Guest
POSIX failures preserve errno in structured metadata. Transport failures are not
mislabeled as guest failures. Failed background work is logged and surfaced in
runtime status; no promise, task, or SQLite error is silently dropped.

## Readiness and cold start

Readiness has two separate package concepts:

1. **Desired packages** are required by this actor's durable configuration.
   Their verified acquisition is a correctness requirement. Readiness cannot
   succeed if they are missing or invalid.
2. **Predicted hot packages** come from the coordinator. They are an optimization.
   Warming stops at a configured process-start deadline and failures are logged
   and measured without making an unrelated actor permanently unhealthy.

A package already baked into the image or present in the process-local cache is
immediately usable. Cache fills use per-digest single-flight coalescing so many
actors do not acquire the same immutable bytes concurrently.

## PR and revision sequence

1. [Delete the TypeScript hosted actor](./01-delete-typescript-hosted-actor.md)
2. [Strip Core and the runtime](./02-strip-core-and-runtime.md)
3. [Build the Rust actor foundation](./03-build-rust-actor-foundation.md)
4. [Implement filesystem and mounting](./04-implement-filesystem-and-mounting.md)
5. [Implement processes and terminals](./05-implement-processes-and-terminals.md)
6. [Implement language execution and contexts](./06-implement-language-execution-and-contexts.md)
7. [Implement networking, previews, and scheduling](./07-implement-networking-previews-and-scheduling.md)
8. [Implement URL-based software installation](./08-implement-url-based-software-installation.md)
9. [Implement the process-local package cache](./09-implement-process-local-package-cache.md)
10. [Implement the preload coordinator](./10-implement-preload-coordinator.md)
11. [Implement dynamic configuration](./11-implement-dynamic-configuration.md)
12. [Prototype the TypeScript contract generator](./12-prototype-agentos-bindgen.md)
13. [Delete the stale agentOS Apps copy](./13-delete-stale-agentos-apps.md)
14. [Migrate integrations and publishing](./14-migrate-integrations-and-publishing.md)

Post-project systems are tracked in
[follow-up work](./15-follow-up-work.md): the name/version package registry, the
optional actor-filesystem package source, real Rivet SQLite adapter, large-file
streaming, graceful restart behavior, and possible RivetKit bindgen
generalization.

## Cross-cutting acceptance criteria

- The hosted actor is Rust-only and registered as `agentOS`.
- Actor action handlers are thin translations to a shared Core API.
- Agents, sessions, ACP, Actor Runtime Socket, and their schemas are absent from
  default builds and publications.
- The TypeScript Core embedded API still supports host bindings.
- No hosted action accepts a host path, callback, closure, or VFS instance.
- Actor creation config and full-document replacements survive actor restart.
- All storage owners use independent schemas and migrations.
- TypeScript and Rust clients are generated from or checked against one wire
  contract and remain behaviorally identical.
- Every queue, stream, replay buffer, cache, list, timeout, and bulk operation is
  bounded by default with typed failure behavior.
- Cheap workspace checks pass after each PR; targeted runtime and integration
  tests are added with the layer that introduces the behavior.

## Resolved MVP decisions

1. **Uninstall semantics:** uninstalling an absent exact package id returns the
   typed `software_not_found` error.
2. **Package replacement:** reinstalling the same digest is idempotent. A
   different package that collides on a projected package or command name is
   rejected rather than replacing existing content implicitly.
3. **Private URLs:** durable config accepts stable HTTPS URLs. Expiring URLs and
   renewable registry authentication remain follow-up registry work.
4. **Artifact signing:** the MVP relies on HTTPS plus the expected SHA-256 digest.
   Signed release manifests remain follow-up work.
5. **Package format:** runtime packages use `.aospkg` v2 without agent metadata;
   v1 support and documentation were deleted with no compatibility path.
6. **Preload source:** an operator baseline and the bounded observed hot set are
   combined. Either source is advisory and may be stale.
7. **Preload ranking:** the coordinator uses a bounded recency-weighted exact map
   with deterministic eviction, not a probabilistic sketch.
8. **Filesystem descriptors:** the hosted whitelist contains `default` and
   `actor-sqlite` roots plus `actor-sqlite` mounts. Unknown backends and every
   host path are rejected during config normalization.
9. **Event naming:** public events use dotted names.
10. **Binary transport:** RivetKit CBOR carries byte strings as `Uint8Array`;
    generated outputs permit `number | bigint` for 64-bit values.
11. **Actor action limits:** every action family has its own bounded byte, item,
    concurrency, replay, stream, process, or terminal policy. These limits are
    independent of RivetKit's type-level action-count limit.
12. **Remaining adapters:** Eve and Flue use the generated sandbox-only client;
    Gigacode and the stale agent/session harnesses were removed.
13. **agentOS bindgen schema:** the prototype is product-local, exports the Rust
    action/event registry, and generates dotted nested TypeScript actions. A
    generic RivetKit bindgen remains optional follow-up work.

## Primary concerns

- Deleting the TypeScript actor first intentionally creates a period where the
  hosted product does not build. The branch stack and CI expectations must make
  that explicit rather than masking it with a compatibility actor.
- The current actor owns significant output replay, terminal emulation, preview,
  scheduling, dynamic mounts, and linked-software behavior. Each behavior must
  move to the owning shared layer or a narrow Rust adapter; deletion alone is not
  proof that Core already covers it.
- Core's local-path source must be impossible to construct through actor
  deserialization. Sharing one source enum without a closed actor-specific DTO
  would recreate the host-filesystem vulnerability.
- Arbitrary URL fetching in a trusted process creates SSRF and credential-leak
  risk. The downloader needs strict scheme, redirect, egress, size, deadline,
  ambient-credential, and logging policy even though package code itself remains
  untrusted guest content.
- URL text is mutable and cannot be the content cache key. The first successful
  installation must pin a digest so actor restart never silently upgrades bytes.
- Health cannot wait indefinitely for package acquisition. Required package failure
  must be distinguishable from optional preload timeout, and both require
  observable metrics and typed state.
- A single physical SQLite file can tempt cross-owner migrations. Adapter and
  test boundaries must enforce namespace ownership from the first PR.
- Generated TypeScript types alone do not guarantee behavioral parity. Shared
  protocol fixtures and cross-client conformance tests are still required.
- Removing ACP touches more than the actor package: protocol frames, sidecar
  extensions, adapters, schemas, publish discovery, examples, docs, and CI can
  keep dead functionality reachable unless absence is explicitly tested.
- The old `@rivet-dev/agentos-apps` package is already represented by the separate
  `@rivet-dev/dynamic-apps` project. Step 13 deletes the stale copy as its own
  revision; migration of Dynamic Apps itself remains outside this repository.
