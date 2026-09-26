import { AgentOs } from "@rivet-dev/agentos-core";

// Operator-provided storage plugins are an embedded Core capability.
export const vm = await AgentOs.create({
	mounts: [
		{
			path: "/mnt/data",
			plugin: {
				id: "s3",
				config: {
					bucket: "my-bucket",
					prefix: "agent-data/",
					region: "us-east-1",
				},
			},
		},
	],
});
