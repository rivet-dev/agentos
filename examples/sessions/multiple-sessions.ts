import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Create two sessions in the same VM
const coder = await agent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
const reviewer = await agent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// Coder writes code
await agent.sendPrompt(coder, "Write a REST API at /home/agentos/api.ts");

// Reviewer reads and reviews the same file
await agent.sendPrompt(reviewer, "Review /home/agentos/api.ts for issues");

// Close each session independently
await agent.closeSession(coder);
await agent.closeSession(reviewer);
