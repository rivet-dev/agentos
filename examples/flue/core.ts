import { createAgent } from "@flue/runtime";
import { AgentOs } from "@rivet-dev/agentos-core";
import { agentOSCoreSandbox } from "@rivet-dev/agentos-flue";

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-4-6",
	sandbox: agentOSCoreSandbox({
		create: ({ id }) =>
			AgentOs.create({
				mounts: [
					{
						path: "/workspace",
						plugin: {
							id: "host_dir",
							config: {
								hostPath: `/var/lib/flue/${encodeURIComponent(id)}`,
							},
						},
						readOnly: false,
					},
				],
			}),
	}),
}));
