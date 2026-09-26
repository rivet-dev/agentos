# 01: Delete the TypeScript Hosted Actor

**Status:** Implemented

## Outcome

Remove the existing configurable TypeScript RivetKit actor before building its
Rust replacement. This prevents the migration from preserving actor-owned policy,
legacy aliases, callbacks, and session behavior merely because they already
exist.

This revision is expected to make the hosted actor temporarily unavailable in
the stacked branch. Embedded `@rivet-dev/agentos-core` remains present and is not
cleaned up until step 02.

## Scope

- Delete `packages/agentos/src/actor.ts` and its exports.
- Delete the TypeScript actor client wrappers whose contract is inferred from
  that actor implementation. They will be regenerated from Rust in step 12.
- Delete actor-only runtime state such as VM maps, output subscriptions, shell
  replay ownership, preview-token state, dynamic mount replay, and linked
  software replay.
- Delete actor lifecycle options and hooks, including `resolveOptions`,
  `onVmStart`, `onVmStop`, and `onVmDisposed`.
- Delete flat compatibility action aliases.
- Delete actor tests, fixtures, and type tests that exercise the removed
  TypeScript actor.
- Delete inspector UI code that exists only for agent/session/permission-prompt
  behavior. Preserve unrelated reusable UI only if a current consumer remains.
- Remove actor-specific build entry points and package exports from
  `@rivet-dev/agentos` without inventing a temporary compatibility package.
- Remove actor examples and internal test harness wiring that can only boot the
  deleted implementation.

## Preserve

- `@rivet-dev/agentos-core` and its embedded API.
- Host bindings in Core.
- Runtime, kernel, VFS, language execution, registry software, and generic
  sidecar code.
- Package publishing infrastructure that will later publish the generated
  TypeScript client and Rust binary, unless it directly assumes TypeScript actor
  source exists.
- Documentation useful for describing intended sandbox behavior. Public pages
  that instruct users to import the deleted actor must be removed or clearly
  excluded from publication until the replacement lands.

## Removal inventory

The revision must explicitly eliminate these old hosted surfaces:

- `createAgentOsActor` and `createAgentOsActions`.
- `AgentOsActorDefinition`, `AgentOsActorExtras`, and derived action types.
- `AgentOsClient` wrappers coupled to the old action tree.
- `agents.*`, `sessions.*`, and session event subscription helpers.
- Flat actions such as `readFile`, `exec`, `openShell`, `vmFetch`, and
  `createPreviewUrl`.
- Actor Runtime Socket fixtures used only by hosted actor tests.
- Actor-owned SQLite tables and migrations for sessions, mounts, or linked
  software. The new Rust actor will create a fresh `agentos_actor_*` schema.
- The 512 MiB actor message-size override and any other broad transport limit
  introduced to make unbounded operations fit.

## Implementation rules

- Do not move behavior from `actor.ts` into another TypeScript file.
- Do not leave deprecated stubs that throw at runtime.
- Do not preserve action names solely for compatibility. There is no hosted API
  compatibility requirement.
- If a non-actor package imports an actor-only symbol, remove or isolate that
  consumer. Do not re-export the symbol from Core.
- If a test proves shared Core behavior, move it to the Core or Rust client test
  suite before deleting the actor test. Tests of actor wiring are simply removed
  and later replaced by Rust integration tests.

## Acceptance criteria

- No TypeScript source defines a RivetKit agentOS actor.
- No default build imports `packages/agentos/src/actor.ts` or its action types.
- No TypeScript actor lifecycle callback remains public.
- No hosted actor test silently switches to an embedded Core test.
- The root dependency graph has no dead actor-only packages or build tasks.
- A repository search documents all intentional remaining references to the old
  actor, limited to migration specifications or historical changelog material.
- Cheap builds pass for packages that are expected to remain buildable at this
  point. Any intentionally broken hosted integration is listed in the revision
  description and restored by a named later step.

## Validation

- Run TypeScript build and type checks for unaffected packages.
- Run Core unit tests.
- Search for the deleted actor constructors, lifecycle hooks, flat aliases, and
  old actor schema names.
- Inspect the package graph to confirm Turbo does not schedule removed actor
  entry points.

## Dependencies and follow-up

This is the first revision. Step 02 removes agent/session/ACP and Actor Runtime
Socket code from the retained Core and native layers. Step 03 introduces the new
Rust actor rather than restoring any TypeScript implementation.
