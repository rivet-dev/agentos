---
title: "Crash Course"
description: "Guided tour through core capabilities: sessions, filesystem, processes, networking, cron, permissions, and multiplayer."
category: "Quickstart"
order: 15
---

# Crash Course

A guided tour through the core capabilities of Agent OS, one small client per feature. Reach for it when you want to see the whole surface area — sessions, filesystem, processes, networking, cron, permissions, and multiplayer — without reading the full docs.

## How it works

A single `server.ts` stands up an Agent OS registry with the `pi` agent software, and each `*-client.ts` file connects to it to exercise one capability:

- **Sessions** (`minimal-client.ts`, `sessions-client.ts`) — create a session, stream `sessionEvent`s, send prompts, and read back files the agent wrote.
- **Filesystem** (`filesystem-client.ts`) — `writeFile`, `readFile`, and recursive directory listing.
- **Processes** (`processes-client.ts`) — one-shot `exec` plus long-running `spawn` with streamed `processOutput`.
- **Networking** (`networking-client.ts`) — `vmFetch` against an in-VM service and signed public preview URLs.
- **Cron** (`cron-client.ts`) — schedule recurring `exec` commands and agent sessions.
- **Permissions** (`permissions-client.ts`, `permissions-server.ts`) — handle permission requests client-side (human-in-the-loop) or auto-approve server-side.
- **Multiplayer** (`multiplayer-client.ts`) — two clients observing the same shared agent session.
- **Agent-to-agent** (`agent-to-agent-*.ts`) — a coder agent calls a `review` binding that drives a separate reviewer agent.

## Run it

```bash
npm install
npx tsx server.ts          # start the registry
npx tsx minimal-client.ts  # then run any client in another terminal
```

Each client prints its results — streamed events, file contents, process output, or URLs — to the console.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/crash-course
