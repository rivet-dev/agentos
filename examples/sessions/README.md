---
title: "Sessions"
description: "Create, manage, and stream agent sessions over the RivetKit actor client."
category: "Sessions & Permissions"
order: 1
---

Spin up an agent VM, open sessions against it, and drive them end to end: send prompts, stream responses, switch models, and tear sessions down. Reach for this when you need full lifecycle control over an agent rather than a one-shot prompt.

## How it works

The server registers an agent VM with `agentOS({ software: [pi] })` and exposes it through a typed RivetKit `setup` registry. The client connects with `createClient` and grabs a VM handle with `getOrCreate`. From that handle, `openSession` creates or restores a durable session using options such as `env`, `cwd`, `mcpServers`, and `additionalInstructions`; `prompt` and `cancelPrompt` drive turns. A `connect()` connection surfaces live `sessionEvent`, `vmBooted`, and `vmShutdown` events. Completed ACP updates are retained in SQLite and can be recovered with `readHistory`; streaming deltas remain ephemeral. Use `getSession` and `listSessions` without starting an adapter, `getSessionConfig` and `setSessionConfigOption` for adapter-defined controls, `unloadSession` to release only the runtime, and `deleteSession` to permanently remove a session and its history.

## Run it

```bash
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # start the registry
# in another shell: drive the client functions
ANTHROPIC_API_KEY=sk-... npx tsx client.ts
```

The server boots the agent VM and the client opens sessions, streams events, and prints session IDs and responses.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/sessions
