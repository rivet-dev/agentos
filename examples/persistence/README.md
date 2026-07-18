---
title: "Persistence"
description: "Filesystem persistence and VM sleep/wake lifecycle management."
category: "Sessions & Permissions"
order: 7
---

VMs sleep when idle and wake on demand while files under `/home/agentos`, the durable session catalog, and completed session history remain in actor SQLite. Reach for this when agent work must survive client disconnects, actor sleep, or long gaps between turns.

## How it works

The server registers a VM with `agentOS({ software: [pi] })` and `setup`. On the client, native RivetKit `connect().on("vmBooted", ...)` and `connect().on("vmShutdown", ...)` subscriptions expose lifecycle events; the shutdown payload's `reason` (`"sleep"`, `"destroy"`, or `"error"`) tells you why the VM stopped. Files, durable session data, dynamic mount descriptors, and linked software descriptors are stored in actor SQLite and replayed when the VM wakes.

An active prompt turn keeps the actor awake through its terminal SQLite commit; an idle session does not. Sleep discards adapter processes, running processes, shells, subscriptions, and incomplete message deltas. A storage-only session list or history read wakes the VM without starting an ACP adapter. The next prompt for a durable session restores the adapter lazily, while completed history remains available from SQLite.

## Run it

```sh
npm install
npx tsx examples/persistence/server.ts   # terminal 1: start the registry
npx tsx examples/persistence/lifecycle-client.ts   # terminal 2: watch boot/shutdown events
npx tsx examples/persistence/restore-filesystem.ts  # later: verify persisted files
```

The lifecycle client logs `VM is ready` then shutdown reasons; the restore client reads a file created before the actor slept.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/persistence
