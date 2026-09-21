import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";
import { createClient } from "@rivet-dev/agentos/client";
import { z } from "zod";

// The reviewer is its own isolated agent VM.
const reviewer = agentOS({ software: [pi] });

// The coder gets a `review` host-function collection it can call itself: it copies a file from the
// coder's VM into the reviewer's VM and asks the reviewer to review it.
const coder = agentOS({
	software: [pi],
	hostFunctions: {
		review: {
			submit: {
				inputSchema: z
					.object({ path: z.string() })
					.describe("Submit a file path for review by the reviewer agent."),
				execute: async ({ path }: { path: string }) => {
					const client = createClient<typeof registry>({
						endpoint: "http://localhost:6420",
					});
					const content = await client.coder
						.getOrCreate("feature-auth")
						.filesystem.readFile(path);
					const reviewerHandle = client.reviewer.getOrCreate("feature-auth");
					await reviewerHandle.filesystem.writeFile(path, content);
					await reviewerHandle.sessions.open({
						agent: "pi",
						env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
					});
					const result = await reviewerHandle.sessions.prompt({
						content: [
							{ type: "text", text: `Review ${path} for security issues` },
						],
					});
					return {
						review:
							result.message?.content
								.filter((block) => block.type === "text")
								.map((block) => block.text)
								.join("") ?? "",
					};
				},
			},
		},
	},
});

export const registry = setup({ use: { coder, reviewer } });
registry.start();
