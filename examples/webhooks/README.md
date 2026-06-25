---
title: "Webhooks"
description: "Receive inbound webhooks (e.g. Slack) over Hono and dispatch them to an agent through a queue."
category: "Orchestration"
order: 4
---

Wire an external service's webhooks into an agent. Reach for this when a third party (Slack, GitHub, Stripe) POSTs events to you and you want an agent to react — without blocking the webhook response on the agent's work.

## How it works

A small [Hono](https://hono.dev) server exposes a `/slack/events` endpoint that handles Slack's URL verification handshake and then enqueues each inbound message onto a RivetKit `queue`. A `slackWorker` actor drains that queue, and for every message it spins up an Agent OS session, prompts the agent with the message text, and posts the reply back to Slack via the chat API. Decoupling the HTTP handler from the worker keeps webhook responses fast and lets agent runs proceed asynchronously.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... SLACK_BOT_TOKEN=xoxb-... npx tsx server.ts
```

The server listens for Slack events; each incoming message is queued, answered by the agent, and replied to in-channel.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/webhooks
