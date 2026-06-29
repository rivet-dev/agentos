import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

// The VM does not inherit the host process.env. LLM provider keys are passed
// explicitly per session, so the server just declares the agent software here.
const vm = agentOS({
	software: [pi],
});

export const registry = setup({ use: { vm } });

registry.start();
