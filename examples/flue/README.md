---
title: "Flue"
description: "Run a Flue agent with a durable agentOS sandbox."
category: "Integrations"
order: 2
---

# Flue

Run a Flue agent with agentOS as its sandbox. Flue owns the agent runtime;
agentOS supplies the isolated VM and durable `/workspace` filesystem.

## Run it

```sh
pnpm install
ANTHROPIC_API_KEY=... pnpm dev
```

`flue dev` starts Flue's native router and the Rivet target. The first sandbox
operation lazily starts the shared agentOS registry in the same process and
waits for it to become ready, so there is no second development server.

Connect to the agent:

```sh
npx flue connect assistant local
```

Ask it to create and inspect a file with both filesystem and shell tools:

> Write `hello from Flue` to `/workspace/hello.txt`, run `wc -c
> /workspace/hello.txt`, then read the file back.

Disconnect, reconnect to `assistant/local`, and ask it to read the file again.
The same Flue context reconnects to the same agentOS actor, so the actor-owned
workspace survives sleep, process restarts, and client reconnects.

## Configuration

- Change the actor name passed to `agentOSSandbox()` when your registry uses a name other than `vm`.
- Configure software, permissions, and resource limits on `agentOS()` in `actors.ts`.
- Keep files that must persist under `/workspace`.

See the [Flue integration guide](https://agentos-sdk.dev/docs/frameworks/flue)
for the complete setup.

## Source

View the source on GitHub: https://github.com/rivet-dev/agentos/tree/main/examples/flue
