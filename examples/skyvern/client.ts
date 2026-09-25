import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

// The session connects to Skyvern's hosted MCP server over HTTP. The browser
// runs in Skyvern Cloud, and the API key travels in the `x-api-key` header.
await agent.sessions.open({
	agent: "claude",
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	mcpServers: [
		{
			type: "http",
			name: "skyvern",
			url: "https://api.skyvern.com/mcp/",
			headers: [{ name: "x-api-key", value: process.env.SKYVERN_API_KEY! }],
		},
	],
});

const response = await agent.sessions.prompt({
	content: [
		{
			type: "text",
			text: "Use the Skyvern tools to open https://example.com, click the link on the page, and tell me the title of the page that opens. Close the browser session when you are done.",
		},
	],
});
console.log(response.message?.content ?? []);

await agent.sessions.delete();
