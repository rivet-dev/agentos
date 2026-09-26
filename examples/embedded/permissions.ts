import { AgentOs, type Permissions } from "@rivet-dev/agentos-core";

// Embedded Core accepts the same kernel permission tree as the hosted actor,
// plus policies for embedded-only host functions and mounts.
const permissions = {
	network: {
		default: "deny",
		rules: [
			{
				mode: "allow",
				operations: ["*"],
				patterns: ["dns://api.example.com", "tcp://api.example.com:*"],
			},
		],
	},
} satisfies Permissions;

const vm = await AgentOs.create({ permissions });

await vm.dispose();
