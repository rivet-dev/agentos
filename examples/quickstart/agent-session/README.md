---
title: "Agent Session"
description: "Project Pi into a VM, create an agent session, and send a prompt."
category: "Quickstart"
order: 12
---

Run the Pi coding agent inside an Agent OS VM, send it a prompt, and print its reply.

## How it works

Pass the Pi software package to `AgentOs.create({ software: [pi] })` so the sidecar can resolve the `pi` agent name from the projected package. The example requires `ANTHROPIC_API_KEY`, forwards it in the session environment, prints the final response text, and cleans up the session and VM with `try`/`finally`.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx index.ts
```

Expected: the script prints Pi's response to the prompt.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/agent-session
