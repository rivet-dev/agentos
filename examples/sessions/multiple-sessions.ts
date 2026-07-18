import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

// Create two sessions in the same VM
const coderSessionId = "coder";
await agent.openSession({
	sessionId: coderSessionId,
	agent: "pi",
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
const reviewerSessionId = "reviewer";
await agent.openSession({
	sessionId: reviewerSessionId,
	agent: "pi",
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// Coder writes code
await agent.prompt({
	sessionId: coderSessionId,
	content: [{ type: "text", text: "Write a REST API at /workspace/api.ts" }],
});

// Reviewer reads and reviews the same file
await agent.prompt({
	sessionId: reviewerSessionId,
	content: [{ type: "text", text: "Review /workspace/api.ts for issues" }],
});

// Unload each adapter independently while retaining both histories.
await agent.unloadSession({ sessionId: coderSessionId });
await agent.unloadSession({ sessionId: reviewerSessionId });
