import { agentOS, setup } from "@rivet-dev/agentos";
import { actor, queue } from "rivetkit";
import { createClient } from "@rivet-dev/agentos/client";
import { Hono } from "hono";
import pi from "@agentos-software/pi";

// Actor that processes Slack messages via a queue
const slackWorker = actor({
  queues: {
    messages: queue<{ channel: string; text: string; user: string }>(),
  },
  run: async (c) => {
    const agentHandle = c.client<typeof registry>().vm.getOrCreate("slack-agent");

    for await (const message of c.queue.iter()) {
      const { channel, text, user } = message.body;

      const sessionId = await agentHandle.createSession("pi", {
        env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
      });
      const result = await agentHandle.sendPrompt(
        sessionId,
        `Slack message from ${user} in #${channel}:\n\n${text}\n\nRespond helpfully.`,
      );
      await agentHandle.closeSession(sessionId);

      // Post the response back to Slack
      await fetch("https://slack.com/api/chat.postMessage", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${process.env.SLACK_BOT_TOKEN}`,
        },
        body: JSON.stringify({ channel, text: result.text }),
      });
    }
  },
});

const vm = agentOS({
  software: [pi],
  additionalInstructions: "You answer Slack messages concisely.",
});

export const registry = setup({ use: { slackWorker, vm } });
registry.start();

// Hono server to receive Slack webhooks
const app = new Hono();
const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

app.post("/slack/events", async (c) => {
  const body = await c.req.json();

  // Handle Slack URL verification
  if (body.type === "url_verification") {
    return c.json({ challenge: body.challenge });
  }

  // Queue the message for the agent
  if (body.event?.type === "message" && !body.event?.bot_id) {
    await client.slackWorker.getOrCreate("main").send("messages", {
      channel: body.event.channel,
      text: body.event.text,
      user: body.event.user,
    });
  }

  return c.json({ ok: true });
});

export default app;
