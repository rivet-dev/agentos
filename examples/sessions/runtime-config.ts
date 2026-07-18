import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

await agent.openSession({
	agent: "pi",
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

const config = await agent.getSessionConfig();
console.log(config.options);

await agent.setSessionConfigOption({
	configId: "model",
	value: "anthropic/claude-sonnet-4",
});
