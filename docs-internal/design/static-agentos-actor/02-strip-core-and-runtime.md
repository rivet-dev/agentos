# 02: Strip Core and the Runtime

**Status:** Implemented

## Implementation notes

- The ACP wrapper sidecars, Actor Runtime Socket client, agent/session APIs,
  adapters, agent-only software, and the obsolete browser actor wrapper are
  deleted.
- Core persistence now uses the temporary async local SQLite adapter with
  owner-specific migrations, namespace checks, bounded operations, and clean
  close semantics.
- Packed software uses the compatibility-free v2 manifest. The runtime does not
  decode v1.
- `agentos-native-sidecar` is the only remaining native Core runtime artifact;
  its fd 0/stdout/fd 3 transport and low-level ownership scopes remain generic
  infrastructure.
- Native-sidecar progress-lane tests use the generic VM-database service as
  ordinary queue load after ACP deletion and reserve progress capacity for
  every configured in-flight request.
- Embedded TypeScript Core retains host bindings and host mounts. Neither will
  be admitted by the hosted actor contract.
- The stacked workspace is expected to keep failing only in the explicitly
  deferred agentOS Apps and downstream TypeScript actor integrations until
  steps 13 and 14 remove or migrate them.

## Outcome

Turn the retained agentOS implementation into a sandbox-only Core before the new
hosted actor depends on it. Remove agents, sessions, ACP, and Actor Runtime Socket
from TypeScript Core, Rust clients, protocols, sidecars, storage, packages,
examples, tests, and publication.

Host bindings remain supported by embedded TypeScript Core. The removal target
is the agent/session orchestration plane and the Actor Runtime Socket, not the
trusted host-callback feature available to an in-process embedding.

## Scope

### TypeScript Core

- Remove `agents` and `sessions` APIs and all session types.
- Remove ACP connection, prompt, permission, cancellation, history, capability,
  and agent-info handling.
- Remove agent adapters and discovery from Core.
- Remove agent/session callbacks, limits, state machines, subscriptions, and
  background tasks.
- Remove Actor Runtime Socket database and scheduling drivers.
- Preserve filesystem, process, terminal, execution-context, JavaScript,
  TypeScript, Python, networking, software, cron-command, permissions, limits,
  sidecar, and host-binding APIs.

### Protocol and Rust clients

- Remove agent, session, and ACP request, response, event, callback, and shutdown
  variants from every shared protocol.
- Remove corresponding Rust client methods and conformance fixtures.
- Remove ACP-specific sidecar extensions. The generic native sidecar must expose
  everything required by the sandbox surface.
- Remove Actor Runtime Socket protocol and client crates when no other product
  uses them.
- Keep the native process transport's fd 0, stdout, and fd 3 lanes for generic
  sidecar requests, responses, events, heartbeats, and typed shutdown. Actor
  Runtime Socket removal does not authorize regressing that transport.

### Runtime, persistence, and scheduling

- Remove session stores, agent stores, history stores, pending permission state,
  and their migration ladders.
- Remove session prompt scheduled actions. Embedded Core may preserve its
  in-process generic command scheduler, while the hosted actor will use
  RivetKit's durable cron API.
- Introduce a temporary local SQLite adapter for the remaining Core durable
  state.
- Give the adapter a narrow async interface that can later be backed by Rivet
  SQLite without changing Core callers.
- Use only `agentos_core_*` tables and `agentos_core_schema_version`, all `STRICT`.
- Do not read, migrate, rename, or delete filesystem-owned or actor-owned tables.
- Do not adopt any legacy session or shared component-version tables.

### Registry and package model

- Remove agent enumeration and resolution from `/opt/agentos`.
- Remove runtime dependence on agent adapter metadata.
- Remove agent-only registry packages and build scripts.
- If eliminating agent fields changes the packed `.aospkg` layout or manifest,
  add a package format version. Do not rewrite the published v1 format in place,
  and do not keep a legacy reader in the new runtime.
- Preserve software package resolution and command linking under
  `/opt/agentos/pkgs` and `/opt/agentos/bin`.

### Repository surface

- Remove ACP-only SDK packages, adapters, examples, inspector panels, test
  fixtures, docs routes, publish discovery, and CI jobs.
- Remove agent/session APIs from both public clients in the same revision.
- Remove generated artifacts and lockfile entries made unreachable by the
  deletion.
- Update public architecture documentation so agentOS is described as runtime,
  filesystem, execution, networking, and registry software, not an agent/session
  orchestration product.

## Host bindings boundary

The following remain valid in embedded TypeScript Core:

- Registering trusted host callbacks through the existing bindings API.
- Routing guest binding requests to those callbacks.
- Enforcing binding argument/result limits, timeouts, and error propagation.

The following are removed:

- Any binding transported through Actor Runtime Socket.
- Any assumption that a hosted actor can persist or reconstruct a callback.
- Any hosted actor configuration field for bindings.

Tests must prove embedded host bindings still work after Actor Runtime Socket and
ACP are gone.

## Temporary SQLite adapter

The adapter should express capabilities, not local file mechanics. At minimum it
needs transactions, parameterized execute/query, schema initialization, and
clean close behavior. Callers must not depend on a filesystem path, WAL pragma,
connection type, or local locking behavior.

The adapter must:

- Serialize or pool access with a fixed bound.
- Propagate every operation failure.
- Use a transaction for each migration plus version update.
- Reject attempts to operate outside `agentos_core_*` in Core migrations.
- Support deterministic temporary databases in tests.

## Deliberately removed APIs

- Agent discovery, enumeration, lookup, and execution.
- Session open, get, list, delete, unload, prompt, cancel, permission response,
  history, config, capabilities, and agent info.
- ACP messages, adapters, SDK bridges, and sidecar extension.
- Session event broadcasts and session-specific cron actions.
- Actor Runtime Socket database, scheduling, callback, and control operations.

## Acceptance criteria

- Repository-wide searches find no live agent/session/ACP public API or protocol
  variant.
- No default Cargo or pnpm build includes an ACP or Actor Runtime Socket crate or
  package.
- Generic sidecar startup and shutdown work without Actor Runtime Socket.
- Embedded TypeScript Core host-binding tests pass.
- Filesystem, process, terminal, language, networking, software, and command
  scheduling conformance tests still pass through both TypeScript and Rust
  clients where those surfaces already exist.
- Core owns only `agentos_core_*` schema objects and can initialize against a
  database containing independent filesystem and actor schemas.
- Publish discovery excludes removed packages and artifacts.
- Public docs contain no runnable agent/session/ACP examples.

## Validation

- `cargo check --workspace`
- `pnpm build`
- `pnpm check-types`
- Targeted Core, sidecar, protocol, host-binding, and SQLite migration tests.
- Publish helper and fixed-version checks.
- Repository searches for `ACP`, Actor Runtime Socket identifiers, session
  protocol variants, session tables, and public `agents` or `sessions` methods.

## Risks

- "Agent" appears in the product name and ordinary prose. Absence checks must
  target API identifiers and owned concepts instead of blindly banning the word.
- Generic native transport and Actor Runtime Socket are different layers. Delete
  the latter without removing the fd 3 response/control lane required by the
  native sidecar architecture.
- Some current package metadata combines commands and agents. Decide package
  format versioning before changing packed manifests.
- agentOS Apps and downstream adapters may not compile once sessions disappear.
  The stale Apps copy is deleted in step 13 and supported downstream consumers
  migrate in step 14. The root workspace build strategy must explicitly handle
  the stacked interval.

## Dependencies and follow-up

Depends on step 01 so there is no TypeScript hosted actor consuming the removed
APIs. Step 03 builds the Rust actor only after this sandbox-only Core boundary is
stable.
