# Sessions & Persistence

How agentOS, ACP, RivetKit actors, and durable session persistence fit together.

<Note>These internal architecture docs are mostly generated and maintained by LLMs, then reviewed by humans. They are intentionally verbose; use your preferred LLM to ask focused questions about the architecture as needed.</Note>

agentOS runs coding agents inside VMs and talks to them through the Agent
Communication Protocol (ACP). RivetKit wraps those VMs in durable actors, so a
session can survive actor sleep/wake even though the live VM and agent process
do not.

## Layers

agentOS session architecture has four layers:

| Layer | Responsibility |
| --- | --- |
| RivetKit actor | Owns the public API and durable actor-local SQLite state. |
| agentOS client | Thin facade used by the actor to create sessions, prompt agents, and call the sidecar. |
| agentOS sidecar ACP extension | Launches ACP adapters inside the VM, speaks JSON-RPC, handles permissions, and owns resume orchestration. |
| ACP adapter / agent | Runs inside the VM and speaks ACP over stdio. |

The actor is durable. The VM is disposable. The ACP agent process is live state
inside the VM.

## API Shape

The actor-facing session API is:

- `createSession(agentType, options)`
- `sendPrompt(sessionId, text)`
- `closeSession(sessionId)`
- `listPersistedSessions()`
- `getSessionEvents(sessionId)`

`sessionId` is the stable, client-facing id. If fallback resume creates a new
live ACP session id after wake, the actor keeps an internal
`externalSessionId -> liveSessionId` remap. Clients keep using the original
`sessionId`.

## Create Flow

1. The actor calls agentOS `createSession`.
2. The sidecar starts the ACP adapter process inside the VM.
3. The sidecar sends ACP `initialize`.
4. The sidecar sends ACP `session/new`.
5. The actor persists session metadata in `agent_os_sessions`.
6. The actor starts capturing ACP `session/update` events for the session.

Persisted session metadata includes:

- `session_id`
- `agent_type`
- agent capabilities and agent info
- create-time `cwd`
- create-time `env`

The create-time `cwd` and `env` are used later so resumed sessions start with
the same working directory and environment they were created with.

## Prompt Flow

1. The actor receives `sendPrompt(sessionId, text)`.
2. If the session is persisted but not live in the current VM, the actor lazily
   resumes it first.
3. The actor writes a synthetic `user_prompt` event before forwarding the
   prompt.
4. The actor forwards the prompt to the live ACP session id.
5. The sidecar sends ACP `session/prompt`.
6. Inbound ACP `session/update` events are captured into
   `agent_os_session_events`.

`agent_os_session_events` is ordered per session. Sequence numbers are allocated
inside the SQLite insert so concurrent prompt and stream captures cannot reuse
the same sequence number.

## Sleep And Wake

When a RivetKit actor sleeps:

- the VM is destroyed
- ACP adapter processes exit
- the actor's in-memory `live_sessions` remap is lost
- actor SQLite survives

When the actor wakes:

- a fresh VM boots
- stable session ids still exist in `agent_os_sessions`
- no ACP session is live yet
- resume happens lazily on the next prompt

## Resume Flow

On the first post-wake prompt for a persisted session:

1. The actor reads `agent_os_sessions`.
2. The actor reconstructs a Markdown transcript from
   `agent_os_session_events`.
3. The actor writes the transcript to
   `/root/.agentos/threads/<sessionId>.md`.
4. The actor calls sidecar `resumeSession` with:
   - stable external `sessionId`
   - agent type
   - transcript path
   - persisted create-time `cwd`
   - persisted create-time `env`

The sidecar then chooses one of two resume paths.

### Native Resume

If the ACP agent advertises `loadSession` or `resume`, the sidecar sends
`session/load` or `session/resume`.

When native resume succeeds:

- the live ACP id is the stable external `sessionId`
- the agent restores its own context
- no transcript preamble is injected

OpenCode uses this path when its own session store is still available in the
durable VM filesystem.

### Transcript Fallback

If native resume is unsupported, or if native resume reports a normalized
`unknown_session`, the sidecar falls back to a fresh session:

1. The sidecar sends ACP `session/new`.
2. The sidecar returns the new live ACP id to the actor.
3. The actor stores `externalSessionId -> liveSessionId`.
4. The sidecar prepends a one-shot preamble to the next prompt pointing at the
   transcript path.

The fallback is universal because it only requires the agent to read a file with
its normal tools. It is lower fidelity than native resume because the transcript
is pointed to, not automatically loaded into the agent's context window.

## Unknown Session Normalization

Adapters report missing sessions differently. The sidecar normalizes known
missing-session shapes into:

```json
{ "error": { "data": { "kind": "unknown_session" } } }
```

For example, OpenCode currently reports a missing native session as:

```json
{ "code": -32603, "data": { "details": "NotFoundError" } }
```

That shape is captured before normalization in tests, then normalized so the
resume state machine can safely choose transcript fallback. Other internal
errors still propagate as failures.

## Persistence

Durable session state lives in actor SQLite:

| Table | Purpose |
| --- | --- |
| `agent_os_sessions` | Stable session registry, agent type, capabilities, agent info, create-time `cwd`, and create-time `env`. |
| `agent_os_session_events` | Append-only prompt and ACP event log keyed by the stable external `sessionId`. |

The transcript file is not canonical state. It is a disposable render of
`agent_os_session_events`, rebuilt on demand during fallback resume.

## What Is Durable

| Data | Survives sleep/wake? | Notes |
| --- | --- | --- |
| Actor SQLite | Yes | Stores session registry, events, preview tokens, and other actor data. |
| VM filesystem | Yes, when backed by the actor sqlite_vfs root | Used by agents and resume transcripts. |
| Live ACP process | No | Recreated on wake. |
| Actor in-memory vars | No | Includes the live ACP id remap. |
| Client-facing `sessionId` | Yes | Stored in `agent_os_sessions`. |

## Where To Look In Code

- Sidecar ACP orchestration:
  `crates/agentos-sidecar/src/acp_extension.rs`
- agentOS TypeScript client surface:
  `packages/core/src/agent-os.ts`
- RivetKit actor session actions:
  `rivetkit-rust/packages/rivetkit-agent-os/src/actions/session.rs`
- RivetKit persistence helpers:
  `rivetkit-rust/packages/rivetkit-agent-os/src/persistence.rs`