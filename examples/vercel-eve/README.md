---
title: "Vercel Eve"
description: "Run a Vercel Eve agent with a durable agentOS sandbox."
category: "Integrations"
order: 1
---

# Vercel Eve

Run a Vercel Eve agent with agentOS as its sandbox backend. Eve owns the agent runtime; agentOS supplies the isolated VM and durable `/workspace` filesystem.

## Run it

```sh
pnpm install
AI_GATEWAY_API_KEY=... pnpm dev
```

`eve dev` starts the Eve server. Eve loads `agent/instrumentation.ts`, which
starts the agentOS registry in the same process, so there is no second
development server. Reconnect to the same Eve session to verify that the
actor-owned workspace survives sandbox sleep and resume.

## Configuration

- Change the actor name passed to `agentOSBackend()` when your registry uses a name other than `vm`.
- Configure software, permissions, and resource limits on `agentOS()` in `registry.ts`.
- Keep files that must persist under `/workspace`.

agentOS is the default in this example, but Eve accepts any compatible sandbox backend. Changing the sandbox does not require changing the agent or selecting a different World.

See the [Vercel Eve integration guide](https://agentos-sdk.dev/docs/frameworks/vercel-eve) for the complete setup.

## Source

View the source on GitHub: https://github.com/rivet-dev/agentos/tree/main/examples/vercel-eve
