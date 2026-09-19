---
title: "Host Functions"
description: "Expose host functions to the agent as CLI commands with Zod-typed inputs."
category: "Reference"
order: 3
---

Give an agent access to your own host code, such as API calls, database lookups, and internal services, through type-safe inputs and an auto-generated CLI surface inside the VM.

## How it works

A host-function collection bundles a `name`, a `description`, and a map of named `functions`. Each function declares a Zod `inputSchema`, an `execute` handler that runs on the host, and optional `examples`. Pass collections to `agentOS({ hostFunctions: [...] })`; agentOS exposes each collection as `/bin/agentos-{name}` inside the VM. When an agent invokes a function, its schema validates the arguments before the handler executes on the host.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts
# in another terminal:
npx tsx client.ts
```

The agent receives the prompt, calls the `weather` forecast host function, and answers using the host-side result.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/host-functions
