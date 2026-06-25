---
title: "Pi Extensions"
description: "Write a custom Pi extension into the VM and have Pi discover and load it at session start."
category: "Quickstart"
order: 13
---

# Pi Extensions

Drop a custom Pi extension into the VM before a session starts and let Pi discover, load, and run it automatically. Reach for this when you want to register tools, reshape the system prompt, or hook agent lifecycle events without forking the agent.

## How it works

Pi scans `~/.pi/agent/extensions/` and `<cwd>/.pi/extensions/` for `.js` files at session start. Each file exports a default factory that receives Pi's `ExtensionAPI`, so it can register tools, subscribe to lifecycle events, and mutate the system prompt.

This example boots a VM with the Pi software bundle, writes a small extension into `/home/agentos/.pi/agent/extensions/` that hooks `before_agent_start` to prepend a mandatory `EXTENSION_OK:` prefix to the system prompt, then creates a Pi session and prompts it. If the reply carries the prefix, the extension was loaded and applied.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx index.ts
```

Expected: the agent answers and prefixes its response with `EXTENSION_OK:`, printing `SUCCESS — Pi extension loaded and modified the system prompt.`

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/pi-extensions
