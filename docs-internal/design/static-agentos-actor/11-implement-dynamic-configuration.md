# 11: Implement Dynamic Configuration

**Status:** Implemented for the MVP. Real Rivet deployment reconciliation and
forced boot-failure coverage remain in step 14.

## Outcome

Complete the durable desired-configuration API for the static actor. Creation
and later mutation use the same input type and normalization path, which
delegates the VM document to Core validation. Mutation replaces the complete
config document; it is not a merge patch, JSON Patch, or list of field
operations.

## Actions

- `config.get`
- `config.set`
- `runtime.restart`

`config.set` is the only generic configuration mutation. Filesystem, preview,
permission, and runtime helpers do not maintain independent config slices or
perform hidden read-modify-write operations.

`software.install` and `software.uninstall` are explicit domain mutations of the
software set defined in step 08. They update the same config revision and record;
they do not create a second source of truth.

## Configuration shape

The final creation and replacement schema covers:

- VM user.
- Base environment.
- Remote `.aospkg` URL sources and resolved digests.
- Allowed Node.js built-ins.
- High-resolution time policy.
- Hosted root filesystem descriptor and mount list.
- Permissions.
- Resource limits.
- Preview policy.

The concrete preview policy contains `defaultTtlMs`, `maxTtlMs`, and
`maxActive`. Defaults are 15 minutes, 24 hours, and 128 active leases. The
actor enforces a one-second minimum TTL and hard-caps caller policy at the
actor-owned 24-hour/128-lease ceilings.

It deliberately excludes:

- Host bindings.
- Host paths and host mounts.
- Database implementation and file paths.
- Download credentials, ambient authentication, and egress policy.
- Cache and preload coordinator settings.
- Sidecar executable or transport configuration.
- Lifecycle callbacks.
- Schedule-driver implementation.
- Runtime-internal handles.

Operator-only settings remain immutable process configuration.

## Replacement contract

```ts
interface SetConfigInput {
  expectedRevision?: number;
  config: AgentOsActorConfigInput;
}

interface AgentOsConfigSnapshot {
  revision: number;
  desired: CompleteAgentOsActorConfig;
  appliedRevision: number | null;
  state: "applying" | "ready" | "restart_required" | "failed";
  issues: ConfigIssue[];
}
```

Every property of `CompleteAgentOsActorConfig` has a defined default. Core owns
VM defaults; the actor owns only its preview-gateway defaults.
`AgentOsActorConfigInput` permits omission for ergonomics, but omission means
"reset this property to its default," not "preserve the stored value."

For fields whose default is selected inside Core, the complete Rust document
retains an explicit default marker such as `environment: null`; it does not
copy the base environment into actor state. An explicit empty environment map
remains distinct and is forwarded as an empty VM environment. The Rust and
TypeScript Core APIs now both expose this complete-environment override, and
the Rust Core mirror now forwards the existing high-resolution-time option.

For example, if the stored config has environment variables and two mounts,
calling `config.set({ config: { limits: newLimits } })` resets the environment,
mounts, and every other omitted property to their defaults. The normal safe
workflow is:

1. Read the complete normalized document with `config.get`.
2. Modify the desired properties locally.
3. Send the complete result to `config.set` with `expectedRevision`.

Explicit empty arrays, empty maps, `false`, and zero-valued fields are preserved
when valid. There is no `null` deletion convention and no special array merge
behavior.

The software actions are a deliberate narrow exception to generic full-document
replacement. `software.install` adds one resolved digest idempotently;
`software.uninstall` removes one exact package id. Both accept
`expectedRevision`, update the same normalized record, and return the resulting
config snapshot. These explicit domain actions use Core's live link/unlink
operations; generic full-list replacement remains conservative and requires a
runtime restart. They do not expose a general array patch operation.

## Defaults ownership

The actor exposes one normalization entry point equivalent to:

```text
normalize_config(input) -> CompleteAgentOsActorConfig
```

Creation and `config.set` use the same Rust normalization entry point. It
forwards the VM portion through the Rust Core serializer and canonical VM config
validation. The actor owns only hosted-boundary normalization: the closed
actor-SQLite filesystem registry, URL-only software DTO, action-size bounds,
and preview gateway policy. The embedded TypeScript and Rust Core surfaces keep
matching environment and timer semantics; step 12 generates the actor-facing
TypeScript DTO rather than introducing another handwritten normalizer.

## Revision and transaction semantics

1. Normalize and validate the submitted config using Core.
2. Validate hosted-only serialization and security constraints.
3. Resolve and verify any new package URLs required by the candidate config
   without exposing host paths or ambient credentials.
4. Compare actor-only preview policy separately from the Core VM creation
   document. Preview-only changes are live. Any changed Core VM field requires
   replacement because Core does not expose partial live reconfiguration.
5. Under the actor's serialized mutation guard, compare `expectedRevision` when supplied and
   commit the complete normalized config with a monotonic revision.
6. Apply preview policy immediately and advance the applied revision only when
   the previous Core revision was ready.
7. For a changed Core VM document, leave `restart_required` explicit until
   `runtime.restart` replaces the runtime under the separately specified
   restart policy.

The optional revision is checked once before remote package work and again
under the mutation guard. Thus a known-stale write fails cheaply, while two
writers that started from the same revision cannot both commit. Without an
expected revision, writers commit serially and the last complete document wins.

Required URL packages are resolved with the process cache before commit, at a
maximum concurrency of eight and a 60-second whole-replacement deadline.
Resolved size and package-id metadata accepted from a `config.get` round trip is
never trusted: the resolver verifies and rewrites it before persistence.

`expectedRevision` is optional optimistic concurrency control. Without it, last
writer wins for the entire document. With it, a stale writer receives
`config_conflict` and no state changes.

The actor never claims atomicity between the SQLite commit and an external
runtime mutation. Desired state is the durable authority. After a crash, startup
compares desired and applied revisions and reconciles again idempotently.

## Creation behavior

Actor creation accepts `AgentOsActorCreateInput { config? }`. RivetKit passes it
to `create_state` only on first creation. Invalid input aborts that creation with
the typed error thrown by the actor.

The actor normalizes the supplied config, or an empty input, and stores the
complete result as revision 1. Once state exists, later `getOrCreate` input is
ignored by ordinary RivetKit state semantics. It is not compared with stored
config and never acts as an update.

Creation state is necessarily synchronous, so initial URL resolution happens
during bounded Core boot. A successful boot pins the same verified resolution
metadata back into the durable revision without incrementing it.

## Restart behavior is deferred

This step identifies when a config requires runtime replacement and exposes that
fact through `config.get` and `runtime.status`. It does not define graceful
process draining, signals, terminal shutdown, or force-kill timing. Those
semantics are tracked as follow-up work.

Until that follow-up lands, `runtime.restart` uses the existing bounded Core
runtime replacement primitive and reports failure accurately. No spec in this
series should imply stronger graceful-shutdown guarantees.

If boot fails, preserve desired config, report failed reconciliation, and allow a
later corrected replacement or explicit restart. Do not silently fall back to
an old configuration while reporting success.

## Health and readiness

`runtime.status` and platform health expose:

- Desired and applied revisions.
- Reconciliation state and restart requirement.
- Required software acquisition state.
- Optional preload state separately.
- Runtime generation and lifecycle state.
- Bounded typed issues with timestamps.

`runtime.status.configState` reports `restart_required` while the old ready VM
continues serving actions, and its desired/applied revisions identify the exact
gap. `ConfigSnapshot` serializes this field as `state`.

Readiness succeeds only when the desired revision is applied and its required
packages are present. It never waits indefinitely. Optional predicted preloads
have their own deadline and do not make an otherwise ready actor fail.

## Implemented tests

- Omitted replacement fields reset environment and timer policy to defaults.
- Explicit empty environment, `false`, minimum preview TTL, and a one-lease
  preview policy remain distinct from omission.
- Preview-only replacement advances the applied revision without replacing Core.
- Core VM changes retain the old applied revision and report
  `restart_required`.
- Environment and permission collections have explicit actor-bound limits.
- Resolved software snapshots round-trip through input while untrusted metadata
  is discarded before resolution.
- Rust Core preserves explicit environment and timer policy, including explicit
  empty/false values and the default base environment.
- The action registry includes `config.set`, and the complete actor unit suite
  guards the URL-only and host-mount-free boundary.

Step 14 must add real Rivet integration coverage for concurrent conditional
writes, process death between desired commit and apply, wake reconciliation,
and a forced replacement-boot failure. Those require a running actor database
and sidecar rather than a unit-only mock of durable state.

## Acceptance criteria

- Creation and mutation use one versioned input type and normalization
  implementation, which delegates VM validation to Core.
- `config.set` replaces one complete normalized desired document.
- Omitted properties reset to defaults; no field or array is incrementally
  patched.
- Software install/uninstall are bounded exact-set operations over that same
  desired document, not a second package state store.
- Desired config and its revision survive every actor/process restart.
- Optimistic concurrency prevents silent overwrite when callers opt into it.
- Core owns VM defaults and validation. The actor owns only its hosted security
  boundary and recognizes its own preview-only live policy; all changed Core VM
  fields conservatively require replacement.
- Every committed revision is either applied or visibly failed/pending.
- Actor health distinguishes required package work from optional preloading.

## Dependencies and follow-up

Depends on all feature layers through step 10 so the reconciler can classify the
whole config. Step 12 generates this final Rust contract for TypeScript.
Graceful restart and process-drain behavior, filesystem-path installation, and
the name/version registry are tracked in step 15.
