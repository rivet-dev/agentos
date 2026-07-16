---
title: "Persistence"
description: "Filesystem persistence and VM sleep/wake lifecycle management."
category: "Sessions & Permissions"
order: 7
---

VMs sleep when idle and wake on demand while files under `/home/agentos` remain durable. Reach for this when agent work must survive client disconnects, actor sleep, or long gaps between turns.

## How it works

The server registers a VM with `agentOS({ software: [pi] })` and `setup`. On the client, `connect()` surfaces `vmBooted` and `vmShutdown` lifecycle events—the shutdown payload's `reason` (`"sleep"`, `"destroy"`, or `"error"`) tells you why the VM stopped. Files under `/home/agentos` are stored by the sidecar directly in the actor's SQLite database over its authenticated Unix socket. Live sessions and event streams end when the VM shuts down.

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
