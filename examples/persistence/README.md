---
title: "Persistence"
description: "Session persistence: lifecycle management and resuming a session after disconnect."
category: "Sessions & Permissions"
order: 7
---

VMs sleep when idle and wake on demand, so sessions outlive any single connection. Reach for this when an agent needs to survive client disconnects, restarts, or long gaps between turns without losing its transcript.

## How it works

The server registers a VM with `agentOS({ software: [pi] })` and `setup`. On the client, `connect()` surfaces `vmBooted` and `vmShutdown` lifecycle events — the shutdown payload's `reason` (`"sleep"`, `"destroy"`, or `"error"`) tells you why the VM stopped. Sessions are written to durable storage as they run, so even with no VM running you can call `vm.listPersistedSessions()` to enumerate past sessions and `vm.getSessionEvents(sessionId)` to replay a session's ordered event transcript after a disconnect.

## Run it

```sh
npm install
npx tsx examples/persistence/server.ts   # terminal 1: start the registry
npx tsx examples/persistence/lifecycle-client.ts   # terminal 2: watch boot/shutdown events
npx tsx examples/persistence/resume-client.ts       # later: list and replay persisted sessions
```

The lifecycle client logs `VM is ready` then shutdown reasons; the resume client prints prior session counts and replays the latest transcript.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/persistence
