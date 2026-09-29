import claude from "@agentos-software/claude-code";
import { AgentOs } from "@rivet-dev/agentos-core";

const { ANTHROPIC_API_KEY, SKYVERN_API_KEY } = process.env;
if (!ANTHROPIC_API_KEY || !SKYVERN_API_KEY) {
	throw new Error("Set ANTHROPIC_API_KEY and SKYVERN_API_KEY.");
}

// Allow only the model provider and Skyvern's hosted MCP server.
const vm = await AgentOs.create({
	software: [claude],
	permissions: {
		network: {
			default: "deny",
			rules: [
				{
					mode: "allow",
					operations: ["*"],
					patterns: [
						"dns://api.anthropic.com",
						"tcp://api.anthropic.com:*",
						"dns://api.skyvern.com",
						"tcp://api.skyvern.com:*",
					],
				},
			],
		},
	},
});

try {
	await vm.sessions.open({
		agent: "claude",
		env: { ANTHROPIC_API_KEY },
		mcpServers: [
			{
				type: "http",
				name: "skyvern",
				url: "https://api.skyvern.com/mcp/",
				headers: [{ name: "x-api-key", value: SKYVERN_API_KEY }],
			},
		],
	});

	const result = await vm.sessions.prompt({
		content: [
			{
				type: "text",
				text: "Use the Skyvern tools to open https://example.com and tell me the main heading on the page. Close the browser session when you are done.",
			},
		],
	});
	console.log(result.message?.content ?? []);
} finally {
	await vm.dispose();
}
