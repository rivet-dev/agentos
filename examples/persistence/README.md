---
title: "Persistence"
description: "Filesystem persistence and VM sleep/wake lifecycle management."
category: "Configuration"
order: 7
---

VMs sleep when idle and wake on demand while files under `/home/agentos` remain in actor SQLite. Reach for this when work must survive client disconnects, actor sleep, or long idle periods.

## How it works

The generated client resolves the fixed `agentOS` actor. Native RivetKit
`connect().on("vm.booted", ...)` and
`connect().on("vm.shutdown", ...)` subscriptions expose lifecycle events.
Files, filesystem configuration, and installed software descriptors are stored
in actor SQLite and reconciled when the VM wakes.

Sleep discards running processes, terminals, and live subscriptions. Durable
filesystem state remains available after the next action wakes the actor.

## Run it

```sh
npm install
npx tsx examples/persistence/lifecycle-client.ts   # watch boot/shutdown events
npx tsx examples/persistence/restore-filesystem.ts  # later: verify persisted files
```

The lifecycle client logs `VM is ready` then shutdown reasons; the restore client reads a file created before the actor slept.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/persistence
