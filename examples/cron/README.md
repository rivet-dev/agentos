---
title: "Cron"
description: "Schedule recurring commands and agent sessions with cron, including overlap handling, monitoring, and cancellation."
category: "Orchestration"
order: 1
---

Run work on a schedule inside a VM — a shell command on a fixed interval, or a recurring agent session that reviews logs, triages issues, or processes a queue. Reach for this when you need background jobs that fire on a cron expression instead of on demand.

## How it works

Each VM handle exposes `scheduleCron({ schedule, action, overlap })`. The `schedule` is a standard cron expression, and the `action` is either an `exec` (run a command with args) or a `session` (spawn an agent of a given `agentType` with a `prompt`). The `overlap` policy decides what happens when a run is still going when the next tick arrives: `skip` drops the new run, `queue` lines it up to run after. Scheduling returns a job `id` you can later pass to `cancelCronJob`, and `listCronJobs` enumerates everything registered on the VM. Connecting to the handle and listening for `cronEvent` streams each run's lifecycle so you can monitor execution.

## Run it

```sh
npm install
npx tsx server.ts   # start the registry, then run any example, e.g. npx tsx schedule-session.ts
```

You should see the cron job registered and its `id` printed; scheduled runs fire on their interval and surface as `cronEvent`s.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/cron
