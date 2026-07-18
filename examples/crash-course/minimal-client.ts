import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

// Subscribe to streaming events
const conn = agent.connect();
conn.on("sessionEvent", (event) => {
	console.log(event);
});

// Create a session and send a prompt
await agent.openSession({
	agent: "pi",
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
const response = await agent.prompt({
	content: [
		{ type: "text", text: "Write a hello world script to /workspace/hello.js" },
	],
});
console.log(response.message?.content ?? []);

// Read the file the agent created
const content = await agent.readFile("/workspace/hello.js");
console.log(new TextDecoder().decode(content));
