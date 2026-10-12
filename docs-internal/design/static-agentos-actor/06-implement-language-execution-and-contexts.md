# 06: Implement Language Execution and Contexts

**Status:** Implemented; cross-client conformance deferred to step 14

## Outcome

Expose JavaScript, TypeScript, Python, and named execution contexts through
one-for-one Rust actor actions. Language behavior, project installation, process
creation, permissions, timeouts, and context semantics remain in Core.

## Context actions

- `contexts.create`
- `contexts.get`
- `contexts.list`
- `contexts.reset`
- `contexts.delete`

Moving creation from the old top-level `createContext` action into
`contexts.create` makes the contract consistently nested. This is an intentional
breaking change.

Context ids are bounded, normalized by Core, and scoped to the current VM. The
actor returns `{ generation, contextId }` handles and rejects them after a
runtime restart, matching the process and terminal handle model. The actor does
not serialize live language heap state into actor SQLite. Desired configuration
may declare bootstrap contexts later, but interactive context contents are
runtime state.

## JavaScript actions

- `javascript.execute`
- `javascript.evaluate`
- `javascript.executeFile`
- `javascript.spawn`
- `javascript.spawnFile`
- `javascript.npm.install`
- `javascript.npm.runScript`
- `javascript.npm.runPackage`

## TypeScript actions

- `typescript.execute`
- `typescript.evaluate`
- `typescript.executeFile`
- `typescript.spawn`
- `typescript.spawnFile`
- `typescript.check`
- `typescript.checkProject`

## Python actions

- `python.execute`
- `python.evaluate`
- `python.executeFile`
- `python.executeModule`
- `python.spawn`
- `python.spawnFile`
- `python.spawnModule`
- `python.install`

## Contract rules

- Reuse the shared language request, result, diagnostic, and spawn DTOs.
- Preserve omitted fields so Core supplies defaults.
- Represent source and path as distinct fields and actions. The actor must not
  guess whether a string is code or a file.
- Spawn actions return the same process handle shape as `process.spawn` and are
  observed through process actions and events.
- Inline execution returns bounded stdout, stderr, value, diagnostics, and exit
  metadata according to the Core contract.
- Large values and binary data use the shared structured serialization format;
  the actor does not JSON-stringify language values ad hoc.
- Timeouts cancel through Core and report whether the guest execution was
  interrupted. They do not merely stop waiting in the actor.

## Guest npm is separate from registry software

`javascript.npm.*` operates inside the guest project and follows the existing
language-execution API. It may download normal project dependencies into the
guest filesystem subject to network and resource policy.

For the MVP, `javascript.npm.install` uses one bounded request shape. An empty
`packages` list installs the current guest project and permits `frozen`; a
non-empty list installs those packages and permits `dev` and `global`. These
option groups are mutually exclusive.

The software API designed in step 08 downloads standalone agentOS `.aospkg`
artifacts from URLs, caches verified content by digest, and projects them through
VFS under `/opt/agentos`. It does not use npm. The guest package-manager APIs and
registry software config must use different types and error codes so callers
cannot confuse them.

## Security and resources

- All language code and installed third-party packages are untrusted.
- Execution remains on the bounded guest executor, never a Tokio runtime worker.
- Network, filesystem, environment, built-in module, process, memory, and time
  permissions are enforced in Core/sidecar.
- The actor cannot add host bindings. Embedded TypeScript Core bindings continue
  to work and receive their own conformance coverage.
- Source size, result size, diagnostics, dependency counts, install bytes,
  concurrent executions, context count, and per-context retained state are
  bounded with typed errors.

## Tests

- Contract fixtures for every action through Rust Core client and actor.
- Behavioral parity with embedded TypeScript Core.
- Context create/use/reset/delete and stale context behavior after runtime
  restart.
- Spawned language jobs integrate with process output, wait, signal, and replay.
- JavaScript and TypeScript source/file distinction.
- Python source/file/module distinction.
- TypeScript diagnostic serialization.
- Guest npm and Python install policy, timeouts, and byte limits.
- Host bindings continue to work only in embedded Core and are absent from actor
  creation/action schemas.
- Hostile payloads cannot block trusted Tokio workers or escape configured
  permissions.

## Implementation notes

- All 28 actions are individually typed and registered under their dotted
  names. Together with steps 03–05, the actor currently exposes 64 actions.
- Inline calls translate directly into Rust Core options. Captured output,
  evaluation values, errors, and TypeScript diagnostics are converted to a
  camel-case actor DTO and bounded to a 768 KiB encoded result.
- Binary stdin and output use CBOR byte strings. Evaluation inputs, compiler
  options, and structured result values remain structured CBOR/JSON values; the
  actor does not turn the whole result into an opaque JSON string.
- Spawned language executions always enable Core's bounded replay, return the
  same generation-scoped process handle as `process.spawn`, and attach the same
  `process.output` and `process.exit` forwarding path.
- Embedded Rust Core now honors `retain_events: false` for language spawns;
  actor spawns opt in explicitly rather than forcing retention for every Core
  caller.
- Context creation returns its normalized descriptor. Reset returns the
  refreshed descriptor; delete returns no value.
- Source, paths, contexts, arguments, environment, stdin, JSON values, package
  lists, install indexes, timeouts, diagnostics, and encoded results all have
  explicit actor-bound limits.

## Acceptance criteria

- All listed language and context actions are available with dotted names.
- Handler bodies contain transport translation, not language implementation.
- Spawned executions use the shared process model.
- No agent adapter, ACP session, or prompt assembly code returns with language
  support.
- The actor contract contains no binding registration surface.
- TypeScript and Rust Core clients pass the same language fixtures.

## Dependencies and follow-up

Depends on process support in step 05 and filesystem support in step 04. Step 12
generates the nested TypeScript ergonomics after all language DTOs stabilize.
