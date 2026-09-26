import { AgentOs } from "@rivet-dev/agentos-core";

// Host paths can be mounted only by trusted embedded Core callers.
export const vm = await AgentOs.create({
	mounts: [
		{
			path: "/mnt/code",
			plugin: { id: "host_dir", config: { hostPath: "/path/to/repo" } },
			readOnly: true,
		},
	],
});
