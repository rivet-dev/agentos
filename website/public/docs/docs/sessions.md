# Sessions

Create agent sessions, send prompts, stream responses, and subscribe to events.

Sessions launch an agent inside the VM, stream its responses in real time over `sessionEvent`, and persist a replayable ACP transcript that survives sleep/wake.

## Create a session

Use `createSession` to launch an agent inside the VM. Returns session metadata including capabilities and agent info. The agent starts in `/home/agentos` by default; override it with the `cwd` option below.

### `createSession` options

The second argument to `createSession` accepts:

- **`env`**: environment variables for the agent process (e.g. API keys). Not inherited from the host.
- **`cwd`**: working directory inside the VM. Defaults to `/home/agentos`.
- **`mcpServers`**: MCP servers (local child processes or remote URLs) exposing extra tools.
- **`additionalInstructions`**: text appended to the agent's system prompt.
- **`skipOsInstructions`**: skip the base OS instructions injection. Tool documentation is still included.

## Send a prompt

Use `sendPrompt` to send a message to an active session. The response contains the agent's reply.

## Stream responses

Subscribe to `sessionEvent` to receive real-time streaming output from the agent.

## Cancel a prompt

Use `cancelPrompt` to stop an in-progress prompt.

## Close and destroy sessions

- `closeSession` gracefully closes a session without removing persisted data
- `destroySession` removes the session and all persisted data
- To reconnect to a previously created session and replay its history, see [Replay events](#replay-events) and [Resuming a suspended session](/docs/architecture/agent-sessions#resuming-a-suspended-session)

## Runtime configuration

Change model, mode, and thought level on a live session.

## Replay events

Use `getSessionEvents` to replay a session's persisted events, including for VMs that are not currently running. Pair it with `listPersistedSessions` to find earlier sessions.

## Persisted session history

Query session history from SQLite. Works even when the VM is not running.

## Multiple sessions

A single VM can run multiple sessions simultaneously. Each session has its own agent process but shares the same filesystem. Use different session IDs to manage them independently.

## Agent logs

The agent (ACP adapter) runs as a process inside the VM. It uses **stdout** for ACP protocol traffic, so its **stderr** is the channel for logs, warnings, and crash diagnostics. Pass `onAgentStderr` to the VM to capture it, and route it to your own logger to see exactly what the agent is doing (or why it exited).

`onAgentStderr` is a VM-level option, so it covers every session's agent process. It's the fastest way to diagnose an agent that exits unexpectedly mid-turn; the crash reason surfaces here. If you omit it, chunks are written to the host `process.stderr` by default.