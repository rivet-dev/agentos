import { agentOS, setup } from "@rivet-dev/agentos";
import { actor, queue } from "rivetkit";
import pi from "@agentos-software/pi";

const reviewer = actor({
  queues: {
    review: queue<{ file: string }, { summary: string }>(),
  },
  run: async (c) => {
    const agentHandle = c.actors.vm.getOrCreate("reviewer");
    const session = await agentHandle.createSession("claude", {
      env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    });

    for await (const message of c.queue.iter({ completable: true })) {
      const content = await agentHandle.readFile(message.body.file);
      const text = new TextDecoder().decode(content);

      await agentHandle.sendPrompt(
        session.sessionId,
        `Review this code and write a summary to /home/agentos/review.txt:\n\n${text}`,
      );

      const review = await agentHandle.readFile("/home/agentos/review.txt");
      await message.complete({
        summary: new TextDecoder().decode(review),
      });
    }
  },
});

const vm = agentOS({ software: [pi] });

export const registry = setup({ use: { reviewer, vm } });
registry.start();
