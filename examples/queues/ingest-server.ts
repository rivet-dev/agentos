import { agentOS, setup } from "@rivet-dev/agentos";
import { actor, queue } from "rivetkit";
import pi from "./software/pi";

const issueWorker = actor({
  queues: {
    issues: queue<{ title: string; body: string }>(),
  },
  actions: {
    // HTTP endpoint to receive webhook payloads
    ingestIssue: async (c, title: string, body: string) => {
      await c.queue.push("issues", { title, body });
    },
  },
  run: async (c) => {
    const agentHandle = c.actors.vm.getOrCreate("issue-worker");

    for await (const message of c.queue.iter()) {
      const session = await agentHandle.createSession("claude", {
        env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
      });
      await agentHandle.sendPrompt(
        session.sessionId,
        `Investigate and fix this issue:\n\nTitle: ${message.body.title}\n\n${message.body.body}`,
      );
      await agentHandle.closeSession(session.sessionId);
    }
  },
});

const vm = agentOS({ software: [pi] });

export const registry = setup({ use: { issueWorker, vm } });
registry.start();
