---
title: "Bindings"
description: "Expose host functions to the agent as CLI commands via Zod-typed bindings."
category: "Reference"
order: 3
---

Give an agent access to your own host code—API calls, database lookups, internal services—through type-safe inputs and an auto-generated CLI surface inside the VM.

## How it works

A binding collection bundles a `name`, a `description`, and a map of named `bindings`. Each binding declares a Zod `inputSchema`, an `execute` handler that runs on the host, and optional `examples`. Pass collections to `agentOS({ bindings: [...] })`; AgentOS exposes each collection as `/usr/local/bin/agentos-{name}` inside the VM. When an agent invokes a binding, its schema validates the arguments before the handler executes host-side.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts
# in another terminal:
npx tsx client.ts
```

The agent receives the prompt, calls the `weather` forecast binding, and answers using the host-side result.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/bindings
