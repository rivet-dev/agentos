# Webhooks

Trigger agent workflows from external webhooks using Hono and queues.

Use a lightweight HTTP server to receive webhooks and drive an agent. This example uses [Hono](https://hono.dev) to receive Slack webhooks and call an agent directly.

## Example: Slack webhook to agent

## How it works

1. Slack sends an HTTP POST to `/slack/events`
2. The Hono handler validates the event and pushes it to the actor's queue
3. The queue processes messages one at a time, creating agent sessions for each
4. The agent responds and the worker posts the reply back to Slack

The queue provides backpressure and durability. If the agent is busy, messages wait in the queue. If the server restarts, queued messages are replayed.

## Recommendations

- Return `200` from the webhook handler immediately after queuing. External services like Slack have short timeout windows.
- Store webhook secrets in environment variables, not in code.