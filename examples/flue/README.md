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

`flue dev` starts the Flue server. The first sandbox operation lazily starts the
shared agentOS registry in the same process and waits for it to become ready, so
there is no second development server. Reconnect with the same Flue agent ID to
verify that the actor-owned workspace survives sandbox sleep and resume.

## Configuration

- Change the actor name passed to `agentOSSandbox()` when your registry uses a name other than `vm`.
- Configure software, permissions, and resource limits on `agentOS()` in `registry.ts`.
- Keep files that must persist under `/workspace`.

See the [Flue integration guide](https://agentos-sdk.dev/docs/frameworks/flue)
for the complete setup, including how to run the Flue agent itself on Rivet.

## Source

View the source on GitHub: https://github.com/rivet-dev/agentos/tree/main/examples/flue
