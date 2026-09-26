import { createAgentOsClient } from "@rivet-dev/agentos";

const client = createAgentOsClient({ endpoint: "http://localhost:6420" });

export const vm = client.agentOS.getOrCreate(["examples", "filesystem"], {
	createWithInput: {
		config: {
			filesystem: {
				mounts: [
					{
						path: "/home/agentos/scratch",
						backend: { type: "actor-sqlite", namespace: "scratch" },
					},
				],
			},
		},
	},
});
