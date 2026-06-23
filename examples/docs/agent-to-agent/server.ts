import { agentOS, setup } from "@rivet-dev/agentos";
import { createClient } from "@rivet-dev/agentos/client";
import { z } from "zod";

// The reviewer is its own isolated agent VM.
const reviewer = agentOS({});

// Bridge the writer to the reviewer: read a file from the writer's VM, copy it
// into the reviewer's VM, and ask the reviewer to review it. Runs on the host.
async function reviewFile(path: string): Promise<string> {
  const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
  const writerHandle = client.writer.getOrCreate("my-project");
  const reviewerHandle = client.reviewer.getOrCreate("my-project");

  // Read file from writer, write to reviewer.
  const content = await writerHandle.readFile(path);
  await reviewerHandle.writeFile(path, content);

  // Ask the reviewer to review.
  const session = await reviewerHandle.createSession("claude", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  const result = await reviewerHandle.sendPrompt(
    session.sessionId,
    `Review the code at ${path} and list any issues.`,
  );
  await reviewerHandle.closeSession(session.sessionId);

  return result.text;
}

// The writer agent gets a `review` binding. When the writer runs
// `agentos-review submit --path ...`, the bridge above executes on the host.
const writer = agentOS({
  bindings: [
    {
      name: "review",
      description: "Send a file to the reviewer agent and get back a review.",
      bindings: {
        submit: {
          description: "Submit a file for review by the reviewer agent.",
          inputSchema: z.object({
            path: z.string().describe("Absolute path of the file to review."),
          }),
          execute: async (input: { path: string }) => ({
            review: await reviewFile(input.path),
          }),
        },
      },
    },
  ],
});

export const registry = setup({ use: { writer, reviewer } });

registry.start();
