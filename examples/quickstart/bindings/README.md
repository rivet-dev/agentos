---
title: "Bindings"
description: "Define host-side bindings callable from inside the VM as CLI commands."
category: "Quickstart"
order: 9
---

Expose host-side functions to code running inside the VM. Reach for this when guest code needs to call back out to capabilities you implement on the host — weather lookups, calculators, database access — without granting it direct host access.

## How it works

You declare binding groups with `bindingGroup`, where each `binding` pairs a Zod `inputSchema` with an `execute` function that runs on the host. Pass the groups to `AgentOs.create({ bindings })` and Agent OS installs CLI commands inside the VM. This example wires up `weather` and `calc`, then invokes each through `agentos-weather` and `agentos-calc`.

## Run it

```bash
npm install
npx tsx index.ts
```

Prints the weather and calculator results returned from the host.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/bindings
