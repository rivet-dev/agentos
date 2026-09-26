import claude from "@agentos-software/claude-code";
import { agentOS, setup } from "@rivet-dev/agentos";

// The VM denies external network access by default. Allow only the model
// provider and Skyvern's hosted MCP server. Each host needs a DNS rule for name
// resolution and a TCP rule for the connection.
const vm = agentOS({
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

export const registry = setup({ use: { vm } });
registry.start();
