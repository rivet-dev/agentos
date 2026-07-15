# Native Plugin Host Overlay Implementation Plan

Status: accepted for implementation

Thread: `019f6472-e8f9-7e03-82ad-2cc0fb1ac2b2`

## Outcome

AgentOS remains a Rust-backed Rivet actor with direct SQLite and VM access,
while gaining the same TypeScript configuration, connection lifecycle,
authorization, custom-action, event-subscription, and callback facilities as a
normal RivetKit actor.

The implementation must produce one actor, one `ActorFactory`, one database,
one connection set, and one lifecycle. TypeScript is an optional host overlay
around the native backend; it is not a second actor and does not proxy native
database or filesystem operations.

Durable session resume is explicitly excluded. It is owned by a separate PR.

## Current Architecture

RivetKit currently chooses one of two mutually exclusive factory paths:

```text
normal actor(...)
  -> buildNativeFactory
  -> NapiActorFactory(callbacks, config)
  -> NAPI event loop invokes TypeScript

nativeFactoryBuilder
  -> createNativePluginFactory
  -> NapiActorFactory::from_native_plugin
  -> CallbackBindings::empty()
  -> dylib pulls and handles every actor event
```

AgentOS constructs a minimal `actor({ actions: {} })` definition and attaches a
`nativeFactoryBuilder`. The marker bypasses normal callback construction, so
the public AgentOS config can accept callbacks that are never bound.

The dylib ABI also makes the plugin own a pull-based event loop. The host moves
each non-cloneable actor reply into a token slab, the plugin pulls an event with
`next_event`, and later completes the token with `reply_ok` or `reply_err`.
Wrapping that flow in TypeScript would require proxy replies and duplicate
cancellation/lifecycle machinery.

## Target Architecture

RivetKit owns the universal actor event loop and pushes events into a native
backend:

```text
ActorDefinition
  -> host callback overlay (optional)
  -> native backend descriptor (optional)
  -> one composed Core ActorFactory

ActorEvent
  -> optional host pre-hook
  -> host action when explicitly registered, otherwise native backend
  -> optional host response hook
  -> original actor reply
```

The generic native-plugin ABI remains independent of TypeScript and AgentOS.
NAPI adapts JavaScript thread-safe functions into generic Rust callback
closures. Existing native plugins may use an empty overlay.

## Ownership Rules

### RivetKit owns

- Actor mailbox admission and event ordering.
- Original reply ownership and exactly-once completion.
- Callback timeouts, cancellation, and shutdown grace.
- Connection parameter validation and connection-state installation.
- Host action routing and native fallback.
- Host pre/post hooks and subscription authorization.
- Loading and ABI validation of native plugins.

### The native AgentOS backend owns

- VM creation and shutdown.
- AgentOS native action implementations.
- Direct actor SQLite access and AgentOS durable tables.
- Filesystem, processes, shells, previews, sessions, and cron behavior.
- Native background workers and event pumps.
- Native sleep/destroy cleanup.

### The TypeScript AgentOS overlay owns

- Per-instance `createOptions(c, input)` evaluation.
- Connection schemas and authentication hooks.
- Derived connection principal/state.
- Pre-action authorization.
- Custom TypeScript actions.
- Subscription policy.
- Server-side session and permission callbacks.
- JavaScript bindings invoked from the native backend.

### Initially unsupported composition conflicts

- Duplicate TypeScript/native action names are rejected.
- Native AgentOS remains the serialized actor-state owner.
- Native AgentOS remains the raw preview HTTP owner.
- A TypeScript `run`/workflow implementation is not composed into the AgentOS
  backend in this change.
- Arbitrary JavaScript filesystem drivers remain unsupported across the native
  boundary; native mount plugins remain supported.

## Event Semantics

| Event | Required order |
| --- | --- |
| Actor start | Validate input, evaluate `createOptions`, create native instance, signal ready |
| Connection preflight | Validate params, `onBeforeConnect`, `createConnState`, native preflight |
| Connection open | Native open, then `onConnect` |
| Connection close | Invoke both host and native cleanup; one failure must not skip the other |
| Subscription | Event `canSubscribe`, then native subscription handling |
| Action | `onBeforeAction`, TypeScript action or native fallback, `onBeforeActionResponse` |
| Sleep/destroy | Stop admission, cancel or drain in-flight work, run host cleanup and native cleanup within one grace deadline |
| Serialization | Native AgentOS owner for this change |

Scheduled actions have no invoking connection. Authorization hooks receive
that absence explicitly and must choose a system-principal policy rather than
receiving a fabricated connection.

## Native Plugin ABI Precursor

### Single exported descriptor

Replace independent `dlsym` lookups with one exported function returning a
versioned function table:

```rust
#[repr(C)]
pub struct PluginApi {
    pub abi_magic: u64,
    pub abi_version: u64,
    pub struct_size: usize,
    pub plugin_init: PluginInitFn,
    pub factory_new: FactoryNewFn,
    pub factory_free: FactoryFreeFn,
    pub instance_new: InstanceNewFn,
    pub handle_event: HandleEventFn,
    pub cancel_event: CancelEventFn,
    pub shutdown: ShutdownFn,
    pub instance_free: InstanceFreeFn,
}
```

The host must continue keeping loaded libraries alive for the process lifetime.
Unloading code while plugin-created tasks or copied function pointers remain is
unsafe.

### Host-driven events

Remove `next_event`, reply tokens, `reply_ok`, and `reply_err` from the plugin
contract. `handle_event` receives one encoded event and completes that event's
callback directly. Plugins may enqueue work internally and complete later.

The host submits events in actor order but may have multiple events in flight.
Shutdown is a barrier: it closes admission, applies the actor grace deadline,
and guarantees that every admitted completion resolves exactly once.

### Per-instance startup data

The instance-start payload carries actor input and the validated result of
`createOptions`. Static package/plugin data may remain on the factory config.

### Plugin-to-host calls

Add one bounded asynchronous host-call primitive to the host vtable:

```rust
host_call(ctx, name, payload, done, user_data)
```

Names are registered when the composed factory is built. Unknown names fail
with a typed error. Payload and response sizes, callback duration, and
concurrent calls are bounded. Host-call cancellation participates in actor
shutdown.

This primitive backs AgentOS permission decisions, session-event observers,
bindings, agent stderr, limit warnings, and future native-originated callbacks.

### ABI compatibility

Any function-table or wire change bumps the exact-lockstep actor-plugin ABI.
The host rejects old plugins with a precise version mismatch. No fallback to an
older dispatch mode is added.

## TypeScript API

The AgentOS definition becomes generic over actor input, connection params, and
connection state:

```ts
agentOS<Input, ConnParams, ConnState>({
  createOptions(c, input) {
    return { /* serializable AgentOS options */ };
  },
  connParamsSchema,
  createConnState(c, params) {
    return { userId, tenantId, roles };
  },
  onBeforeConnect(c, params) {},
  onConnect(c, conn) {},
  onDisconnect(c, conn) {},
  onBeforeAction(c, name, args) {},
  actions: {
    customAction(c, value) {},
  },
  events: {
    permissionRequest: event({ canSubscribe(c) { return canApprove(c); } }),
  },
});
```

`createOptions` is actor-instance configuration. The one-shot
`AgentOs.create(options)` constructor continues accepting only concrete
`AgentOsOptions` and does not accept an option factory.

Authentication secrets are validated at connection time and are not persisted
as connection state. `createConnState` stores a derived principal. The actor key
is the initial tenancy boundary: all admitted connections to one actor share
its native event domain unless an event subscription policy denies access.

## Permission Callback Contract

The server permission hook returns a structured decision:

```ts
type PermissionDecision =
  | { handled: true; reply: "once" | "always" | "reject" }
  | { handled: false };
```

`handled: false` forwards the request to authorized client subscribers. Exactly
one response wins. Duplicate or late responses fail deterministically rather
than being silently ignored.

## Planned jj Revisions

### RivetKit repository

1. `refactor(plugin): load actor dylibs through one API descriptor`
   - Introduce the single exported `PluginApi` descriptor.
   - Preserve current pull dispatch temporarily.
   - Add loader validation and descriptor lifetime tests.

2. `feat(plugin): make native actor dispatch host-driven`
   - Add per-instance start and pushed event handling.
   - Remove the event pull/reply-token slab from the new ABI.
   - Define concurrency, cancellation, startup, and shutdown behavior.

3. `feat(actor): compose host callbacks with native backends`
   - Build callbacks even when a definition has a native backend.
   - Route lifecycle hooks, subscription policy, custom actions, and native
     fallback through one actor factory.
   - Preserve the existing standalone native-plugin path as an empty overlay.

4. `feat(actor): add native pre-action and host-call callbacks`
   - Add `onBeforeAction` to the generic actor API.
   - Add bounded named plugin-to-host callbacks.
   - Propagate caller connection and request metadata.

### AgentOS repository

5. `chore(actor): adopt the host-driven native plugin ABI`
   - Port the AgentOS plugin exports and worker to pushed events.
   - Preserve direct SQLite, VM, action, and event behavior.

6. `feat(actor): resolve options from actor create input`
   - Replace the obsolete pre-create shape with `createOptions(c, input)`.
   - Add input/options validation and startup failure tests.

7. `feat(actor): add connection and action authorization hooks`
   - Add connection param schema/state generics and lifecycle hooks.
   - Add `onBeforeAction` enforcement for every native action.
   - Add event subscription authorization.

8. `feat(actor): compose custom TypeScript actions with native actions`
   - Route registered TypeScript actions through the host overlay.
   - Fall back to the native action contract for other names.
   - Reject collisions and preserve generated native action types.

9. `feat(actor): route native callbacks and bindings through the host`
   - Implement structured session and permission callbacks.
   - Route JavaScript bindings through the generic host-call primitive.
   - Add limits, timeout, error propagation, and shutdown tests.

10. `docs(actor): document native overlays and authentication`
    - Remove interim-runtime/stub language.
    - Update authentication, approvals, multiplayer, bindings, architecture,
      and API examples.
    - Document the actor tenancy boundary and event visibility.

Each revision must be independently described and leave its repository buildable
against the corresponding precursor revision. Cross-repository dependency pins
are updated only to published or otherwise reproducible RivetKit artifacts;
local absolute paths are never committed.

## Validation

### RivetKit

- Actor-plugin ABI unit and loader tests.
- Native plugin lifecycle/action/connection integration tests.
- Existing NAPI actor hook and action tests.
- New composed native-backend tests covering empty and populated overlays.
- Cancellation, dropped-callback, timeout, and shutdown-grace tests.
- TypeScript package type checks and NAPI build.

### AgentOS

- `agentos-actor-plugin` unit and action-contract tests.
- AgentOS package type checks and config-schema tests.
- Authentication example type check and connection rejection/acceptance test.
- Native action authorization and custom TypeScript action integration tests.
- Permission hook handled/forwarded/timeout/duplicate-response tests.
- Binding invocation success, error, timeout, payload-limit, and shutdown tests.
- Existing filesystem, process, shell, preview, cron, persistence, and inspector
  regressions.
- Website build after documentation changes.

### Cross-repository handoff

The AgentOS repository currently consumes published RivetKit preview packages
and an exact actor-plugin ABI crate. Implementation and local validation may use
a clean linked RivetKit build, but the final AgentOS dependency revision must
point to a reproducible published preview rather than a developer filesystem.

## Completion Criteria

- AgentOS config callbacks are either executed or rejected by validation; none
  are silently dropped.
- Native actions retain direct Rust/SQLite/VM execution.
- `createOptions` is evaluated once per actor instance with actor input.
- Connection and action authorization receive the real invoking context.
- Custom TypeScript actions and native actions coexist under one client type.
- Permission callbacks have one structured and bounded response path.
- Bindings use the same generic host-call mechanism.
- Existing empty-overlay dylib actors remain supported.
- Documentation and checked examples describe the implemented behavior.
- Durable session resume code is untouched.
