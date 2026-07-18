---
title: "Webhooks"
description: "Receive inbound webhooks (e.g. Slack) over Hono and dispatch them directly to an agent session."
category: "Orchestration"
order: 4
---

Wire an external service's webhooks into an agent. Reach for this when a third party (Slack, GitHub, Stripe) POSTs events to you and you want an agent to react.

## How it works

A small [Hono](https://hono.dev) server exposes a `/slack/events` endpoint that handles Slack's URL verification handshake, sends each message to one durable AgentOS session, and posts the structured ACP response back through Slack's chat API. AgentOS serializes concurrent prompts for that session automatically; there is no application queue.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-... SLACK_BOT_TOKEN=xoxb-... npx tsx server.ts
```

The server listens for Slack events; each incoming message is answered by the agent and replied to in-channel. This simple example waits for the turn before returning. For providers with short webhook deadlines, acknowledge in infrastructure that offers a durable background-task primitive, then invoke the same AgentOS action there.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/webhooks
