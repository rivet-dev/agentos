import { agentOS, setup } from "@rivet-dev/agentos";
import { actor, queue } from "rivetkit";
import {
  type WorkflowStepContextOf,
  workflow,
} from "rivetkit/workflow";
import pi from "./software/pi";

// The Agent OS VM that each workflow step drives. It is its own actor, kept
// separate from the workflow orchestrator so steps can reach it over the client.
const vm = agentOS({ software: [pi] });

// A durable workflow actor. Its `run` handler is built with `workflow()`, so
// every `ctx.step(...)` is recorded, retried, and resumed independently: if the
// process crashes mid-run, replay skips completed steps and continues where it
// left off. Trigger work by sending to the `fixBug` queue; the workflow loops,
// waiting durably for the next message.
const bugFixer = actor({
  state: {
    lastIssue: null as string | null,
    lastExitCode: null as number | null,
  },
  queues: {
    fixBug: queue<{ repo: string; issue: string }>(),
  },
  run: workflow(async (ctx) => {
    await ctx.loop("fix-bug-loop", async (loopCtx) => {
      // Wait durably for the next bug-fix request.
      const message = await loopCtx.queue.next("wait-fix-bug");
      const { repo, issue } = message.body;

      // Step 1: Clone the repo. Each step is an isolated, retryable unit of
      // work; a crash here resumes from this step on replay.
      await loopCtx.step("clone-repo", (stepCtx) => cloneRepo(stepCtx, repo));

      // Step 2: An agent fixes the bug. The session is created and closed
      // inside the step, so it never has to outlive the work it backs (sessions
      // are ephemeral and would not survive a replay).
      await loopCtx.step("fix-bug", (stepCtx) =>
        fixBugWithAgent(stepCtx, issue),
      );

      // Step 3: Run the tests. The exit code feeds into the next step.
      const exitCode = await loopCtx.step("run-tests", (stepCtx) =>
        runTests(stepCtx),
      );

      // State changes are only valid inside a step callback, so they are
      // recorded as part of replay.
      await loopCtx.step("record-result", async (stepCtx) => {
        stepCtx.state.lastIssue = issue;
        stepCtx.state.lastExitCode = exitCode;
      });
    });
  }),
  actions: {
    getState: (c) => c.state,
  },
});

async function cloneRepo(
  ctx: WorkflowStepContextOf<typeof bugFixer>,
  repo: string,
): Promise<void> {
  const agentHandle = ctx.client<typeof registry>().vm.getOrCreate("bug-fixer");
  await agentHandle.exec(`git clone ${repo} /home/agentos/repo`);
}

async function fixBugWithAgent(
  ctx: WorkflowStepContextOf<typeof bugFixer>,
  issue: string,
): Promise<void> {
  const agentHandle = ctx.client<typeof registry>().vm.getOrCreate("bug-fixer");
  const session = await agentHandle.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  await agentHandle.sendPrompt(
    session.sessionId,
    `Fix the bug described in issue: ${issue}`,
  );
  await agentHandle.closeSession(session.sessionId);
}

async function runTests(
  ctx: WorkflowStepContextOf<typeof bugFixer>,
): Promise<number> {
  const agentHandle = ctx.client<typeof registry>().vm.getOrCreate("bug-fixer");
  const tests = await agentHandle.exec("cd /home/agentos/repo && npm test");
  return tests.exitCode;
}

// Agent chaining: the output of one agent session feeds into the next. Each
// session is created and completed within its own step, and data passes between
// steps through the VM filesystem (a review file) and step return values.
const codeReviewer = actor({
  state: {
    reviewedFiles: 0,
  },
  queues: {
    codeReview: queue<{ filePath: string }>(),
  },
  run: workflow(async (ctx) => {
    await ctx.loop("code-review-loop", async (loopCtx) => {
      const message = await loopCtx.queue.next("wait-code-review");
      const { filePath } = message.body;

      // Step 1: An agent reviews the code and writes findings to a file.
      await loopCtx.step("review", (stepCtx) => reviewCode(stepCtx, filePath));

      // Step 2: Read the review back from the VM filesystem. Its text is the
      // step return value, so it flows into the next step.
      const review = await loopCtx.step("read-review", (stepCtx) =>
        readReview(stepCtx),
      );

      // Step 3: A second session applies fixes based on the review.
      await loopCtx.step("fix", (stepCtx) => applyReview(stepCtx, review));

      await loopCtx.step("record-review", async (stepCtx) => {
        stepCtx.state.reviewedFiles += 1;
      });
    });
  }),
  actions: {
    getState: (c) => c.state,
  },
});

async function reviewCode(
  ctx: WorkflowStepContextOf<typeof codeReviewer>,
  filePath: string,
): Promise<void> {
  const agentHandle = ctx.client<typeof registry>().vm.getOrCreate("reviewer");
  const session = await agentHandle.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  await agentHandle.sendPrompt(
    session.sessionId,
    `Review the code at ${filePath} and write your findings to /home/agentos/review.md`,
  );
  await agentHandle.closeSession(session.sessionId);
}

async function readReview(
  ctx: WorkflowStepContextOf<typeof codeReviewer>,
): Promise<string> {
  const agentHandle = ctx.client<typeof registry>().vm.getOrCreate("reviewer");
  const content = await agentHandle.readFile("/home/agentos/review.md");
  return new TextDecoder().decode(content);
}

async function applyReview(
  ctx: WorkflowStepContextOf<typeof codeReviewer>,
  review: string,
): Promise<void> {
  const agentHandle = ctx.client<typeof registry>().vm.getOrCreate("reviewer");
  const session = await agentHandle.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  await agentHandle.sendPrompt(
    session.sessionId,
    `Apply the following review feedback:\n\n${review}`,
  );
  await agentHandle.closeSession(session.sessionId);
}

export const registry = setup({ use: { vm, bugFixer, codeReviewer } });

registry.start();
