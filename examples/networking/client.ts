import { createAgentOsClient } from "@rivet-dev/agentos";

const client = createAgentOsClient({ endpoint: "http://localhost:6420" });

export const vm = client.agentOS.getOrCreate(["examples", "networking"], {
	createWithInput: {
		config: {
			loopbackExemptPorts: [3000],
			preview: {
				defaultTtlMs: 60 * 60 * 1_000,
				maxTtlMs: 24 * 60 * 60 * 1_000,
				maxActive: 16,
			},
		},
	},
});
