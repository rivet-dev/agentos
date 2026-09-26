---
title: "Processes"
description: "Execute commands, manage processes, and stream terminal events."
category: "Processes & Shell"
order: 1
---

These examples connect to the fixed `agentOS` actor with the generated client.
Actions are nested under `process` and `terminal`; RivetKit connection events
carry live output and exit notifications.

```bash
npm install
npx tsx exec.ts
npx tsx spawn.ts
```

`exec.ts` captures one command. `spawn.ts`, `stdin.ts`, and
`process-events.ts` demonstrate long-running process control and event
subscriptions. `shell.ts` and `shell-events.ts` demonstrate terminals.

The actor process must be available at `http://localhost:6420`.
