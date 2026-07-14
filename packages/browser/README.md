# Agent OS Browser

Browser driver primitives for Agent OS.

- Package: `@rivet-dev/agentos-browser`
- Exports: `createBrowserDriver`, `createBrowserRuntimeDriverFactory`, `createOpfsFileSystem`, `BrowserWorkerAdapter`

## ACP resumable interaction cleanup

The production browser wrapper drives internal ACP pending responses without
exposing them to AgentOs callers. Adapter stderr is submitted back to the
sidecar, which owns the bounded, owner-scoped ACP stderr event queue; TypeScript
does not construct or deliver a parallel local event.

An adapter exit or interaction timeout is routed back to the sidecar as an
authenticated abort request with the originating connection, wire-session, and
VM ownership. The sidecar atomically removes the exact pending core state and
process route, restores any consumed durable prompt preamble, kills/releases the
execution, and returns a stable typed ACP error to the original request. The WASM
frame helpers remain stateless, so they add no second Rust pending map that can be
stranded. Each pending response also carries the sidecar-owned timeout for its
current ACP phase; the TypeScript loop only applies that value and resets it when
the sidecar advances phases.

Worker integrations may provide `isAgentInteractionCancelled` when creating the
converged sidecar. The probe should read host-owned shared state (normally an
`Atomics` flag) so the worker can observe cancellation while polling. TypeScript
forwards only the cancellation fact; the sidecar owns atomic cleanup and the
`agent_interaction_cancelled` result.

## Packed packages

`createAgentOsConvergedSidecar(config, { packageBytes, packagesMountAt })` accepts complete `.aospkg`
artifacts and forwards their bytes opaquely during `initialize_vm`. TypeScript
does not decode manifests, select mount paths, or register commands and agents.
The browser sidecar validates the vbare package metadata, projects read-only
files, applies package environment defaults, and owns replacement, limits, and
rollback. Omit `packageBytes` to preserve omission; pass an empty array only when
an explicit empty package projection is intended.
