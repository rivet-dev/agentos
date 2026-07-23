import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS();

export const registry = setup({
	use: { vm },
});
