# 03: Build the Rust Actor Foundation

**Status:** Implemented; deployment smoke deferred to step 14

## Outcome

Add the prebuilt Rust `agentOS` actor, its process entry point, durable actor
state foundation, generic native sidecar entry point, lifecycle state machine,
and typed action/event registration. This step proves the end-to-end shell of the
new architecture before feature actions are layered on.

## Scope

### Crates and binary

- Add a Rust actor crate containing lifecycle integration, actions, events,
  actor-owned persistence, and Core-client wiring.
- Produce one release binary with explicit entry point selection:
  - Normal mode runs the Rivet `agentOS` actor.
  - Internal sidecar mode runs the generic native sidecar used by Core.
- Select mode from a fixed CLI contract: `actor` runs the hosted actor, while
  `sidecar` (or no argument for Core-client compatibility) runs the internal
  native sidecar. Unknown modes fail before either runtime starts.
  Actor callers cannot choose an arbitrary executable.
- Package the binary as an agentOS release artifact without introducing a second
  version stream.

### Actor lifecycle

- Register the actor under the exact name `agentOS`.
- Accept versioned `AgentOsActorCreateInput` containing optional initial config.
- Open the temporary actor SQLite adapter and migrate `agentos_actor_*` only.
- Normalize and store initial desired config exactly once with revision 1.
- Boot the generic Core VM through the Rust client.
- Track bounded lifecycle states: initializing, preloading, booting, ready,
  degraded, stopping, and failed.
- Shut down Core with a typed deadline and expose failures in state and logs.
- Never hold a Rivet actor request open indefinitely during boot or shutdown.

### Initial actions

- `config.get`
- `runtime.status`
- `runtime.restart`

Feature action names may be registered as typed placeholders only if the Rust
actor framework requires a complete schema at build time. A placeholder must be
unreachable from published clients and must not return a generic "not
implemented" error in a released artifact.

### Initial events

- `runtime.booted`
- `runtime.shutdown`
- `runtime.limitWarning`

### Actor-owned SQLite

The initial schema should be intentionally small:

- `agentos_actor_schema_version`
- `agentos_actor_config` containing desired config, revision, creation timestamp,
  and last update timestamp
- `agentos_actor_runtime_state` only if durable reconciliation needs more than
  the desired config; ephemeral process ids and handles must not be persisted

All tables are `STRICT`. Configuration and revision are written atomically. The
schema must not contain session, ACP, terminal, process, or output history.

### Core boundary

Create a narrow adapter around the Rust Core client so action handlers do not
depend on process launch mechanics. The adapter owns:

- Starting and stopping one VM for the actor.
- Passing a serializable Core configuration.
- Observing sidecar state and forwarding typed events.
- Rejecting calls while the runtime is not in a valid state.

Core remains responsible for actual runtime defaults and enforcement. The actor
calls the shared Core normalizer and persists its complete result; it must not
fill fields from copied actor-owned default constants.

## Thin-handler rule

A normal feature action added in later steps should have this shape:

1. Decode and validate transport bounds.
2. Acquire the current runtime generation.
3. Call exactly one corresponding Core API, or one explicitly named actor
   coordinator for actor-owned behavior.
4. Encode the typed result.

Retries, filesystem semantics, process behavior, and guest policy do not belong
in the handler. Multi-call reconciliation is limited to actor-owned durable
configuration, package acquisition, preview routes, and lifecycle.

## Configuration behavior in this step

The foundation stores the final versioned envelope even though later steps add
validation and reconciliation for individual fields. Initially, only the subset
required to boot the base VM may be accepted. Unsupported fields return a typed
validation error; they are never silently ignored.

Creation follows ordinary RivetKit state semantics:

- `create_state` receives and validates input only when no actor state exists.
- Throwing a typed error from `create_state` aborts invalid first creation.
- Once actor state exists, later `getOrCreate` calls do not reinterpret creation
  input or compare it with stored config.
- Only `config.set` introduced in step 11 changes desired config.

## Runtime status

`runtime.status` is observe-only and does not boot a sleeping or failed VM. It
reports at least:

- Actor lifecycle state.
- Desired and applied config revisions.
- Current runtime generation.
- Last boot and shutdown timestamps.
- Required-package and optional-preload state, even if empty in this revision.
- A bounded list of typed current issues.
- Core sidecar state when a runtime exists.

## Limits

Introduce explicit defaults and operator overrides for:

- Initialization and shutdown deadlines.
- Concurrent action admission.
- Actor action request and response bytes.
- Runtime issue history.
- Event queue capacity.
- SQLite transaction duration or acquisition timeout.

Each threshold warns near capacity and fails with a typed error naming the limit
and operator setting.

## Tests

- Fresh actor creation with no config.
- Fresh actor creation with the supported initial config subset.
- Invalid first-creation input propagates through RivetKit as a typed error.
- Later `getOrCreate` calls do not mutate existing config.
- Restart with config durability.
- Local SQLite migration atomicity and namespace isolation.
- Core boot failure, crash, and shutdown timeout state transitions.
- `runtime.status` does not accidentally boot a VM.
- Bounded concurrent action admission and event overflow behavior.
- Binary entry point selection and rejection of unknown modes.

## Acceptance criteria

- A release build produces one binary capable of normal actor and internal
  generic sidecar modes.
- Rivet can create, wake, inspect, restart, and stop an actor named `agentOS`.
- Initial config and its revision survive process restart.
- No TypeScript actor source participates in execution.
- No Actor Runtime Socket or ACP code is reintroduced.
- The actor schema is independent from filesystem and Core schemas.
- Lifecycle failures are typed, logged, and visible through status.
- The actor can boot an otherwise empty VM through the shared Rust Core client.

## Validation

- Rust formatting, linting, and targeted crate tests.
- `cargo check --workspace`.
- An integration test that launches the compiled binary in both modes.
- A Rivet actor smoke test for create, status, restart, sleep/wake, and shutdown.
- SQLite migration tests against empty and independently populated databases.

## Dependencies and follow-up

Depends on steps 01 and 02. Steps 04 through 10 add feature layers. Step 11
finishes dynamic configuration and reconciliation across those layers. Step 12
publishes the generated TypeScript surface only after the Rust contract is
complete.

## Implementation notes

- The actor lives in `crates/actor` and uses the published Rust RivetKit SDK;
  this project does not modify RivetKit.
- Foundation config supports `user`, `allowedNodeBuiltins`,
  `loopbackExemptPorts`, and `limits`. The closed input rejects later fields
  until their owning feature revision lands.
- Actor configuration is bootstrapped through RivetKit state, then loaded from
  or initialized into the independent `agentos_actor_*` SQLite schema. On wake,
  the SQLite row is authoritative and refreshes the RivetKit state snapshot.
- The proof-of-concept Core database uses an operator-owned local path derived
  from a SHA-256 digest of the actor id. No caller-controlled host path appears
  in the actor contract.
- Targeted actor and executable-entrypoint tests pass. The Rivet deployment
  smoke remains in step 14 because it requires the packaged binary plus the
  deployment configuration owned by that cutover revision.
