---
title: "Sessions"
description: "Create, manage, and stream agent sessions over the RivetKit actor client."
category: "Sessions & Permissions"
order: 1
---

Spin up an agent VM, open sessions against it, and drive them end to end: send prompts, stream responses, switch models, replay history, and tear sessions down. Reach for this when you need full lifecycle control over an agent rather than a one-shot prompt.

## How it works

The server registers an agent VM with `agentOS({ software: [pi] })` and exposes it through a typed RivetKit `setup` registry. The client connects with `createClient` and grabs a VM handle with `getOrCreate`. From that handle you call `createSession` (with options like `env`, `cwd`, `mcpServers`, and `additionalInstructions`), then `sendPrompt`/`cancelPrompt` to run work. A `connect()` connection surfaces `sessionEvent`, `vmBooted`, and `vmShutdown` events for live streaming — subscribe before triggering actions so nothing is missed. Runtime knobs (`setModel`, `setMode`, `setThoughtLevel`), event replay (`getSessionEvents`, `getSequencedEvents`), persisted history, and multi-session fan-out within one VM round out the surface.

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
