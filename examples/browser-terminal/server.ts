import common from "@agentos-software/common";
import { agentOS, setup } from "@rivet-dev/agentos";

const shellVm = agentOS({
	software: [common],
});

export const registry = setup({ use: { shellVm } });

registry.start();
