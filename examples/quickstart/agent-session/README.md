---
title: "Agent Session"
description: "Create an agent session and send a prompt using a coding agent (Pi, Claude, or OpenCode)."
category: "Quickstart"
order: 12
---

Run a coding agent inside an Agent OS VM, send it a prompt, and read back its reply. Reach for this when you want to drive an agent (Pi, Claude, or OpenCode) programmatically rather than through a chat UI.

## How it works

Register the agent software bundles when you `AgentOs.create` a VM, then open a session with `createSession(agent, { env })` for the agent of your choice. Subscribe with `onSessionEvent` to watch streamed text and tool use as it happens, and call `prompt` to send a message and await the final response text. Close the session and dispose the VM when finished. Agents read credentials such as `ANTHROPIC_API_KEY` from the session `env`.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx index.ts
```

Expected: the script prints a session ID, streams session events, and logs the agent's answer (`4`).

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/agent-session
