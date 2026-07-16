import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

const sessionId = await agent.createSession("pi", {
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

await agent.setModel(sessionId, "anthropic/claude-sonnet-4");
await agent.setMode(sessionId, "plan");
await agent.setThoughtLevel(sessionId, "high");
