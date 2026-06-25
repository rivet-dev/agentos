---
title: "Tools"
description: "Define host-side tool kits callable from inside the VM over the tools RPC server."
category: "Quickstart"
order: 9
---

Expose host-side functions to code running inside the VM. Reach for this when guest code needs to call back out to capabilities you implement on the host — weather lookups, calculators, database access — without granting it direct host access.

## How it works

You declare tool kits with `toolKit`, where each `hostTool` pairs a Zod `inputSchema` with an `execute` function that runs on the host. Pass the kits to `AgentOs.create` and the runtime stands up a tools RPC server inside the VM, advertised through the `AGENTOS_TOOLS_PORT` environment variable. Guest Node scripts read that port and `POST` to `/call` with a `{ toolkit, tool, input }` body; the host validates the input, runs `execute`, and returns the result as JSON. This example wires up `weather` and `calc` kits, then invokes each from inside the VM.

## Run it

```bash
npm install
npx tsx index.ts
```

Prints the tools RPC port, then the weather and calculator results returned from the host.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/tools
