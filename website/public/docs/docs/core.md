# Direct VM API

Use the direct AgentOS VM API or layer it into an AgentOS actor.

## Direct VM vs actor

`@rivet-dev/agentos` includes both the direct `AgentOs` VM API and the
`agentOS()` actor API:

| | Direct VM | Actor |
|-|---|---|
| Persistence | In-memory by default (pluggable via [mounts](#mounts)) | Persistent filesystem and sessions |
| Distributed state | Manage yourself | Built-in distributed statefulness |
| Stateful VMs | Complex to run yourself | Built into Rivet |
| Sleep/wake | Manual `dispose()` / `create()` | Automatic |
| Events | Direct callbacks | Broadcasted to all connected clients |
| Preview URLs | None | Built-in signed URL server |
| Multiplayer | N/A | Multiple clients on same actor |
| Orchestration | N/A | Workflows, queues, cron |
| Agent-to-agent communication | Custom | Built into [Rivet Actors](/docs/agent-to-agent) |
| Authentication | Set up yourself | [Documentation](/docs/authentication) |

We recommend using [Rivet Actors](https://rivet.dev/docs/actors) when you need persistence, networking, and orchestration. Use `AgentOs.create()` from the same package when you need direct VM control in a Node.js process.

`agentOS()` returns an ordinary TypeScript Rivet actor definition. Its config accepts the VM options together with normal actor state, actions, events, queues, connection types, and lifecycle hooks such as `onBeforeConnect`. AgentOS actions and events are merged in automatically; their names are reserved so they cannot be accidentally shadowed. After a wake, the actor creates its VM lazily on the first AgentOS action and disposes it on sleep. This lets a connection subscribe before the `vmBooted` event is emitted.

Creation input is inferred from the actor definition and is passed through normal client creation options: `client.vm.create("key", { input })`. The same input reaches `createState(c, input)` and `onCreate(c, input)`.

## Install

```bash
npm install @rivet-dev/agentos
```

## Boot a VM

Create a VM and drive it directly — no actor runtime, no client/server split. `AgentOs.create()` boots the VM in-process and returns a handle you call directly:

## Sidecar process

Every VM runs inside a **shared sidecar process** rather than a process of its own. By default all VMs are tenants of a single, process-global sidecar (the `default` pool), so each additional VM only adds its marginal cost — a V8 isolate plus its kernel state — instead of a whole OS process. This is what keeps per-VM memory in the tens of MB and warm VM creation in the single-digit milliseconds (see [Performance](/docs/performance)).

This is automatic — `agentOS()` and `AgentOs.create()` use the shared default sidecar with no configuration, and the same applies to Rivet Actors (each actor's VM is a tenant of the shared process). Disposing a VM tears down only that VM; the shared sidecar process is reused across VMs and stays alive for the lifetime of the host process.

For advanced cases the direct VM API exposes explicit sidecar handles so you can isolate a group of VMs in their own process:

## Filesystem

## Processes

Portable `spawn()` is callback-free. Subscribe to its unified stdout/stderr stream with `onProcessOutput(pid, …)` and to completion with `onProcessExit(pid, …)`:

## Agent sessions

`openSession` negotiates the adapter and resolves without a value. Omit `sessionId` to use `main`; call `getSession` separately only when you need durable metadata. Native ACP updates and interactive permission request/response variants share the sequenced `onSessionEvent` stream:

Register `onSessionEvent` before prompting to receive live deltas. Durable entries can be recovered with `readHistory`; ephemeral agent/thought deltas cannot.

## Networking

`httpRequest({ port, path, ... })` reaches a server running inside the VM and returns a bounded, serializable response DTO:

## Cron jobs

Cron jobs run an `"exec"` command or a `"session"` prompt on a schedule. Fired jobs are surfaced through the `onCronEvent` callback:

## Mounts

Configure filesystem backends at boot time.

Native mount plugins (host directories, S3, etc.) are passed via `plugin`, each
identified by an `id` and a `config` object.

## Configuration reference

All VM configuration is passed to `AgentOs.create()` as a single flat object. This is the consolidated config block to copy and adapt. The [`agentOS()` actor](/docs/quickstart) accepts the same options and layers persistence, sleep/wake, and preview URLs on top:

The top-level fields are documented inline above. See [Mounts](#mounts) and [Software](/docs/software).

### Session events

With the direct VM API, `onSessionEvent` receives a generic union containing exact native ACP `SessionUpdate`, `RequestPermissionRequest`, and `RequestPermissionResponse` payloads wrapped with AgentOS durability metadata. Register it before prompting. On reconnect, also read durable history after your last sequence and deduplicate by `(sessionId, sequence)`:

### Timeouts and sleep

Action timeouts and automatic sleep/wake are features of the [`agentOS()` actor](/docs/quickstart), not the direct VM API. A direct VM stays alive until you call `dispose()`. See [Persistence & Sleep](/docs/persistence) for the actor's sleep lifecycle.