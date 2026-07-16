---
title: "Sessions & Persistence"
description: "How live ACP sessions and durable actor-backed files fit together."
---

agentOS keeps the session path deliberately thin: the RivetKit actor proxies session actions to the native AgentOS core SDK, and the sidecar owns ACP orchestration inside the VM. Session events are forwarded live to actor connections and native server hooks.

## Runtime layers

| Layer | Responsibility |
| --- | --- |
| RivetKit actor | Actor lifecycle, typed actions/events, preview tokens, and native hooks. |
| AgentOS core SDK | Sidecar transport and public VM/session API. |
| Native sidecar | VM lifecycle, filesystem, ACP processes, permissions, and resource policy. |
| ACP adapter | Agent-specific protocol adapter running inside the VM. |

`createSession` starts an ACP adapter and returns its live session id. `sendPrompt`, permission replies, runtime configuration, and `closeSession` are direct core SDK calls. `sessionEvent` notifications are broadcast as they arrive; the actor does not allocate sequence numbers or build a transcript database.

## Durable filesystem

The VM filesystem is the durable boundary. The sidecar's SQLite VFS implementation connects directly to the actor's authenticated SQLite Unix socket and stores chunked filesystem metadata and data in actor SQLite. SQLite traffic never crosses the TypeScript actor layer.

When an actor sleeps, the VM and every live ACP process stop. On wake, a fresh VM restores `/home/agentos` from actor SQLite. Callers create a new live session and continue from files left by earlier work.

## Live-only events

Register the client `sessionEvent` listener before sending a prompt. Server implementations can use `onSessionEvent` and `onPermissionRequest`; both receive the ordinary actor context, session id, and native payload. Hook promises are retained with actor `waitUntil`, and hook failures are logged.

The actor does not expose transcript replay, event cursors, or persisted session-history actions. Applications that need a durable audit log can store selected events in their own actor state from `onSessionEvent`.

## Code locations

- Actor wrapper: `packages/agentos/src/actor.ts`
- TypeScript core SDK: `packages/core/src/agent-os.ts`
- Native ACP orchestration: `crates/agentos-sidecar/src/acp_extension.rs`
- UDS-backed SQLite VFS: `crates/native-sidecar-core`
