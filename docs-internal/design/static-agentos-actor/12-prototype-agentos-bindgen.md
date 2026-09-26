# 12: Prototype the TypeScript Contract Generator

**Status:** Prototype implemented and integrated; deployed actor smoke remains

## Outcome

Build an agentOS-specific bindgen prototype as its own package. Use the completed
Rust action and event contract as the source of truth and generate the
TypeScript actor types and thin client ergonomics from it. Do not modify
RivetKit in this project.

The prototype restores `@rivet-dev/agentos` as the hosted actor client without
restoring a TypeScript actor implementation. Generalizing the approach for other
RivetKit actors is a possible later project, not an acceptance condition here.

## Implemented prototype

- `packages/agentos-bindgen` owns the deterministic `generate` and `check`
  commands, the committed JSON IR, and the TypeScript emitter.
- A feature-gated Rust exporter derives TypeScript declarations from the real
  actor and Core wire DTOs with `ts-rs`.
- The actor's existing `action_registry!` macro emits both RivetKit action
  registrations and contract entries, with a test requiring the two name sets
  to remain identical.
- Reserved actions remain in the IR as `public: false` and are omitted from the
  generated client.
- `@rivet-dev/agentos` exports the generated create input, nested action tree,
  event map, typed handle, accessor, registry, and vanilla RivetKit client
  factory.
- Type tests cover creation input, nested calls, unit actions, bytes, events,
  and the absence of the internal cron action.

The current RivetKit `BaseActorDefinition` action constraint requires an open
string index signature, which would erase the generated literal action names.
For the prototype, the generated definition uses an empty action record and the
actor handle intersects RivetKit's vanilla handle with the closed generated
action tree. This changes types only; calls still use the vanilla nested actor
proxy and wire protocol.

`ts-rs` represents Rust 64-bit integers as `bigint`. The generated input side
uses `number` because RivetKit's JSON-compatible request encoding does not
accept a JavaScript `bigint`; the output side uses `number | bigint` because
CBOR may decode values outside the safe integer range as `bigint`.

## Package boundary

Add a workspace package at `packages/agentos-bindgen`, named
`@rivet-dev/agentos-bindgen` if it is published. It owns:

- The agentOS contract intermediate representation.
- Rust schema export for agentOS action, event, and error DTOs.
- The TypeScript emitter and agentOS package template.
- A deterministic generate/check command used by the workspace and release
  pipeline.

Keep committed product versions at `0.0.1`. The package may remain private while
it is a build tool; generated `@rivet-dev/agentos` sources may still be committed
for consumers that do not have a Rust toolchain.

The package must not patch, fork, or add features to RivetKit. It can consume the
actor's explicit registration manifest and add agentOS-local schema derives or
descriptors to the actor DTOs.

## Source of truth

The prototype currently exports this subset for every Rust action:

- Exact dotted name.
- Public/internal visibility.
- Request DTO.
- Success DTO.

Every public event contributes its dotted name and versioned payload DTO. The
agentOS actor build exports a deterministic machine-readable schema, and the
bindgen package emits TypeScript from it.

Typed error variants, documentation, and limit metadata remain schema work for
the lockstep conformance phase. The prototype exports the common structured
`RivetError` shape, but it does not yet enumerate each action's possible codes.

RivetKit currently records Rust action names but does not expose request, output,
event, or error schemas. The prototype bridges that gap only for agentOS by
maintaining one agentOS-local contract registry next to action registration. A
test compares its names with RivetKit's registered action set so the two cannot
drift silently.

## Prototype sequence

1. Export the complete registered action set, actor creation input, and event
   set from Rust.
2. Derive dependent DTO declarations and byte/tagged-enum overrides from the
   Rust wire types.
3. Emit a committed JSON IR and generated TypeScript contract.
4. Reconstruct dotted action names as the nested vanilla RivetKit proxy shape.
5. Fail package builds and typechecks when generated files differ.
6. Step 14 adds CBOR byte and integer fixtures, then migrates supported
   consumers to the generated client. A production actor smoke remains part of
   deployment validation.

This is intentionally a prototype with a product-specific registry. Do not add
a handwritten parallel TypeScript contract or make RivetKit generalization part
of the critical path.

## Generated TypeScript shape

Dotted action names become nested properties:

```ts
type AgentOsActions = {
  config: {
    get(): Promise<AgentOsConfigSnapshot>;
    set(input: SetConfigInput): Promise<AgentOsConfigSnapshot>;
  };
  filesystem: {
    readFile(input: ReadFileInput): Promise<ReadFileResult>;
  };
  network: {
    fetchStream: {
      start(input: FetchStreamStartInput): Promise<FetchStreamHead>;
      read(input: FetchStreamReadInput): Promise<FetchStreamChunk>;
      cancel(input: FetchStreamCancelInput): Promise<void>;
    };
  };
};
```

The vanilla RivetKit actor proxy already maps nested property access to dotted
action names. The generated client uses that behavior rather than introducing a
parallel RPC or HTTP API.

## Generated client surface

`@rivet-dev/agentos` exports:

- Generated actor creation config, action request/result, event, status, and
  error types.
- The typed actor definition/handle required by the vanilla RivetKit client.
- Small transport helpers for bytes, streams, fetch-like requests, process event
  subscriptions, and terminal subscriptions where raw actions are not ergonomic.
- Framework bindings only when they are thin subscriptions to the generated
  contract.

It does not export:

- An actor implementation or actor factory.
- VM policy defaults.
- Config reconciliation logic.
- Session or agent abstractions.
- ACP types.
- Host binding registration for the hosted actor.
- Legacy flat action aliases.
- A custom network endpoint that bypasses vanilla actor actions.

Embedded `@rivet-dev/agentos-core` remains separate and keeps its host binding
API. Do not duplicate Core implementation into `@rivet-dev/agentos`.

## Wrapper rules

- A wrapper may translate `Uint8Array`, headers, `Request`, `Response`, async
  iteration, or event callbacks into generated DTOs.
- A wrapper may cancel a stream it opened when the iterator closes.
- A wrapper may detect sequence gaps and call the documented replay action.
- A wrapper may not choose permissions, defaults, mount policy, restart policy,
  retries, package references, or timeout values unless the caller explicitly
  requests them.
- Actor and Core errors retain the generated code and structured details.
- Browser entry points remain disabled unless separately approved.

## Schema checks

There is no backward compatibility obligation. Rust binary, Rust client,
TypeScript client, and protocol crates are released in same-version lockstep.

The prototype checks that:

- Generated files are current and deterministic.
- Every registered action appears in the agentOS schema.
- Internal reserved actions do not enter the public action tree.
- Dotted names reconstruct the expected nested TypeScript tree.
- No action collides with a parent property or another dotted path.
- Removed agent/session/ACP and flat names do not reappear.

Step 14 must add runtime checks that byte encodings, positional arguments,
integer ranges, event payloads, and structured failures round-trip across Rust
and TypeScript.

## Tests

- Focused prototype tests covering registry identity, bytes, JSON values,
  events, dotted path collisions, and nested tree generation.
- Type tests for the complete nested action tree.
- Absence tests for TypeScript actor code and removed APIs.

Rust fixtures cover positional CBOR and native byte strings; generator and type
tests cover integer widening, binary action fields, events, and nested calls.
The final deployed-client smoke test remains a rollout gate, not another local
TypeScript actor harness.

## Acceptance criteria

- [x] `packages/agentos-bindgen` generates the agentOS TypeScript contract.
- [x] No RivetKit repository or package is modified by this project.
- [x] Rust is the single source of public hosted actor DTOs and names.
- [x] The TypeScript client uses vanilla RivetKit actor actions.
- [x] Publication does not require shipping TypeScript actor source.
- [x] The public client is the generated nested vanilla actor proxy and contains
  no runtime policy wrapper.
- [x] Rust CBOR fixtures and TypeScript generator/type fixtures cover the shared
  byte, integer, event, and action shapes.
- [ ] The generated client passes a smoke test against a deployed Rust actor.

## Prototype limitations

- `ts-rs` can identify `Option<T>` fields but cannot infer every
  `#[serde(default)]` omission rule. Public input DTOs should continue to use
  explicit `*Input` types; any new defaulted non-optional input needs an
  explicit schema override or richer IR metadata.
- The schema exports one common structured error shape, not per-action error
  code unions.
- A production transport smoke still requires a deployed Rivet actor; no local
  TypeScript actor is retained solely to simulate that path.

## Dependencies and follow-up

Depends on steps 03 through 11. Prototype representative shapes early enough to
avoid unrepresentable Rust DTOs, then stabilize the complete output only after
the action set is final. Step 14 publishes the generated lockstep artifacts.
Possible RivetKit generalization is tracked separately in step 15 and is not
required for this refactor.
