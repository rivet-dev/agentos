import { agentOS, setup } from "@rivet-dev/agentos";
import { actor, queue } from "rivetkit";
import pi from "@agentos-software/pi";

const taskRunner = actor({
  queues: {
    tasks: queue<{ prompt: string }>(),
  },
  run: async (c) => {
    const agentHandle = c.actors.vm.getOrCreate("task-agent");

    for await (const message of c.queue.iter()) {
      // Process one task at a time
      const session = await agentHandle.createSession("claude", {
        env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
      });
      await agentHandle.sendPrompt(session.sessionId, message.body.prompt);
      await agentHandle.closeSession(session.sessionId);
    }
  },
});

const vm = agentOS({ software: [pi] });

export const registry = setup({ use: { taskRunner, vm } });
registry.start();
