---
title: "Quickstart App"
description: "Full RivetKit app: an agentOS server registry plus a client that streams session events."
category: "Quickstart"
order: 14
---

A complete starting point that wires an agentOS server to a client. Reach for this when you want the whole loop in one place: a server that registers a VM with agent software, and a client that opens a session, sends a prompt, and streams the agent's events back.

## How it works

`server.ts` builds a VM with `agentOS({ software: [pi] })`, registers it via `setup`, and starts the RivetKit registry. The client connects to that registry, calls `getOrCreate` to obtain a VM handle, and subscribes to `sessionEvent` over a live connection. It then creates a `pi` session (passing the Anthropic API key through `env`), sends a prompt, and reads back the file the agent wrote to `/workspace`. An `Agent.tsx` component shows the same flow from React, streaming events into component state with `useEvent`.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # start the registry
ANTHROPIC_API_KEY=sk-... npx tsx client.ts    # in another shell, drive a session
```

The client prints streamed session events and the contents of the `hello.js` file the agent creates.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart-app
