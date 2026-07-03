---
title: "Agent to Agent"
description: "Bridge two isolated agent VMs: a writer agent calls a reviewer agent through a binding."
category: "Agents"
order: 5
---

Run two agents in separate isolated VMs and let one delegate to the other. The writer agent produces code, then hands it to a reviewer agent for feedback — without the two VMs ever sharing a filesystem. Reach for this when you want specialized agents that collaborate but stay isolated.

## How it works

Both agents are independent `agentOS` VMs registered under one `setup`. The writer is given a `review` binding: a host-side function the agent can invoke by name. When the writer runs `agentos-review submit --path ...`, the binding's `execute` runs on the host, where it reads the file out of the writer's VM, copies it into the reviewer's VM, opens a reviewer session, and prompts the reviewer to review the code. The review text is returned to the writer as the binding's result. The two VMs never touch directly — the host bridge is the only path between them.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # start both agent VMs
ANTHROPIC_API_KEY=sk-... npx tsx client.ts    # drive the writer, which calls the reviewer
```

The writer writes an API, submits it through the binding, and the reviewer's feedback comes back inline.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/agent-to-agent
