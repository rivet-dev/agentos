import { agentOS, setup } from "@rivet-dev/agentos";
import { docker } from "@rivet-dev/agentos-sandbox";

const vm = agentOS({
	permissions: {
		fs: "allow",
		network: "allow",
		childProcess: "allow",
		env: "allow",
		binding: "allow",
	},
	sandbox: {
		provider: docker(),
	},
});

export const registry = setup({ use: { vm } });
registry.start();
