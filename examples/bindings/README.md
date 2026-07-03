---
title: "Bindings"
description: "Expose host functions to the agent as CLI commands via Zod-typed bindings."
category: "Reference"
order: 3
---

Give an agent access to your own host code—API calls, database lookups, internal services—without writing a binding from scratch. Reach for bindings when the agent needs to call back into your application and you want type-safe inputs plus an auto-generated CLI surface inside the VM.

## How it works

A binding group bundles a `name`, a `description`, and a map of named bindings. Each binding declares a Zod `inputSchema`, an `execute` handler that runs on the host, and optional `examples`. You pass the groups to `agentOS({ bindings: [...] })`, and Agent OS exposes every group to the agent as a CLI command at `/usr/local/bin/agentos-{name}` inside the VM. When the agent invokes the command, the Zod schema validates the arguments and the handler executes host-side, returning the result back to the guest. The client side stays thin: create a session and send a prompt, and the agent decides when to call the binding.

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
