---
title: "Queues"
description: "Process agent tasks one at a time through a RivetKit queue, with ingest and review pipelines."
category: "Orchestration"
order: 2
---

Run agent work through a durable queue so tasks are handled one at a time instead of all at once. Reach for this when prompts arrive faster than agents should process them — webhook bursts, batch jobs, or any workload where serialized, back-pressured execution beats parallel chaos.

## How it works

A RivetKit `actor` declares a `queue` and drains it inside its `run` loop with `c.queue.iter()`, processing each message sequentially. For every message the actor opens an Agent OS session against a shared VM, sends the prompt, and closes the session — so only one task runs at a time per actor.

The example shows three patterns over the same primitive:

- **Basic** (`server.ts` / `client.ts`) — clients `send` prompts onto the queue; the runner processes them in order.
- **Ingest** (`ingest-server.ts` / `ingest-client.ts`) — an HTTP action `push`es webhook payloads onto the queue for decoupled intake.
- **Review** (`review-server.ts` / `review-client.ts`) — a completable queue (`iter({ completable: true })`) where the client `send`s with `{ wait: true }` and blocks for the agent's returned summary.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # in one terminal
npx tsx client.ts                            # in another
```

Tasks queue up and the agent works through them one at a time; swap in `ingest-*` or `review-*` to try the other pipelines.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/queues
