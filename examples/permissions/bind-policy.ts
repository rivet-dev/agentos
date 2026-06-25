import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	permissions: {
		network: "allow",
		fs: "deny",
	},
});

export const registry = setup({ use: { vm } });
registry.start();
