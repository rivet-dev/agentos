import { agentOS, setup } from "@rivet-dev/agentos";
import { createClient } from "@rivet-dev/agentos/client";
import { z } from "zod";
import pi from "@agentos-software/pi";

// The reviewer is its own isolated agent VM.
const reviewer = agentOS({ software: [pi] });

// The coder gets a `review` toolkit it can call itself: it copies a file from the
// coder's VM into the reviewer's VM and asks the reviewer to review it.
const coder = agentOS({
  software: [pi],
  toolKits: [
    {
      name: "review",
      description: "Send a file to the reviewer agent and get back a review.",
      tools: {
        submit: {
          description: "Submit a file path for review by the reviewer agent.",
          inputSchema: z.object({ path: z.string() }),
          execute: async ({ path }: { path: string }) => {
            const client = createClient<typeof registry>({
              endpoint: "http://localhost:6420",
            });
            const content = await client.coder
              .getOrCreate("feature-auth")
              .readFile(path);
            const reviewerHandle = client.reviewer.getOrCreate("feature-auth");
            await reviewerHandle.writeFile(path, content);
            const sessionId = await reviewerHandle.createSession("pi", {
              env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
            });
            const result = await reviewerHandle.sendPrompt(
              sessionId,
              `Review ${path} for security issues`,
            );
            return { review: result.text };
          },
        },
      },
    },
  ],
});

export const registry = setup({ use: { coder, reviewer } });
registry.start();
