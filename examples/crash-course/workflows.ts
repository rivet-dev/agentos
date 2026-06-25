import { agentOS, setup } from "@rivet-dev/agentos";
import { actor, queue } from "rivetkit";
import { workflow } from "rivetkit/workflow";
import pi from "./software/pi";

const vm = agentOS({ software: [pi] });

// A durable workflow actor. Its `run` is built with `workflow()`, so every
// `step(...)` is recorded, retried, and resumed: if the process crashes
// mid-run, replay skips completed steps and continues where it left off.
const bugFixer = actor({
  queues: {
    fixBug: queue<{ repo: string; issue: string }>(),
  },
  run: workflow(async (ctx) => {
    await ctx.loop("fix-bug-loop", async (loopCtx) => {
      // Wait durably for the next bug-fix request from the queue.
      const message = await loopCtx.queue.next("wait-fix-bug");
      const { repo, issue } = message.body;

      // The typed client is reached from inside a `step` (the only scope with
      // actor data). Each step re-derives the VM handle from `step.client()`.
      await loopCtx.step("clone-repo", (step) =>
        step
          .client<typeof registry>()
          .vm.getOrCreate("bug-fixer")
          .exec(`git clone ${repo} /home/agentos/repo`),
      );

      await loopCtx.step("fix-bug", async (step) => {
        const agent = step.client<typeof registry>().vm.getOrCreate("bug-fixer");
        const sessionId = await agent.createSession("claude", {
          env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
        });
        await agent.sendPrompt(sessionId, `Fix the bug in issue: ${issue}`);
        await agent.closeSession(sessionId);
      });

      await loopCtx.step("run-tests", (step) =>
        step
          .client<typeof registry>()
          .vm.getOrCreate("bug-fixer")
          .exec("cd /home/agentos/repo && npm test"),
      );
    });
  }),
});

export const registry = setup({ use: { vm, bugFixer } });
registry.start();
