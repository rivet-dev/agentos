---
title: "Command Code CLI"
description: "Run the Command Code v1 CLI directly in an agentOS VM."
category: "Agents"
order: 5
---

# Command Code CLI

Run the genuine Command Code v1 CLI inside an agentOS VM. This example invokes `cmd` directly in headless mode; it does not use an ACP adapter.

## Run it

```bash
pnpm install
export COMMAND_CODE_API_KEY=...
pnpm tsx server.ts   # starts the registry on http://localhost:6420
pnpm tsx client.ts   # runs cmd -p and prints its JSON output
```

## Source

View the source on GitHub: https://github.com/rivet-dev/agentos/tree/main/examples/command-code
