import { AgentOs, nodeModulesMount } from "@rivet-dev/agentos-core";

// Host mounts are available only to trusted embedded Core callers.
export const vm = await AgentOs.create({
	mounts: [nodeModulesMount("/absolute/path/to/node_modules")],
});
