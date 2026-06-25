---
title: "Cron"
description: "Schedule, list, and cancel recurring cron commands inside the VM."
category: "Quickstart"
order: 7
---

Run commands on a recurring schedule from inside a VM. Reach for this when you need periodic work — polling, cleanup, heartbeats — driven by the VM itself rather than an external scheduler.

## How it works

Create a VM with `AgentOs.create()`, then call `scheduleCron()` with a cron expression and an `exec` action describing the command to run. The call returns a job whose `id` you keep to manage it later. `listCronJobs()` reports the currently active jobs, and `cancelCronJob(id)` removes one. The example schedules a once-per-second `echo`, lists the active jobs, waits a few seconds for ticks to fire, then cancels and confirms the job is gone.

## Run it

```bash
npm install
npx tsx index.ts
```

You should see the scheduled job id, a list of active jobs, a few cron ticks, and finally a remaining-job count of `0` after cancellation.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/cron
