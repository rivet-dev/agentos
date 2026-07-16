import { agentOS, setup } from "@rivet-dev/agentos";
import {
	createSandboxFs,
	createSandboxBindings,
} from "@rivet-dev/agentos-sandbox";
import { SandboxAgent } from "sandbox-agent";
import { docker } from "sandbox-agent/docker";

const sandbox = await SandboxAgent.start({ sandbox: docker() });

const vm = agentOS({
	// Bindings let the agent control the sandbox
	bindings: [createSandboxBindings({ client: sandbox })],
	// Mounts let the agent read the sandbox filesystem (optional)
	mounts: [
		{
			path: "/home/agentos/sandbox",
			plugin: createSandboxFs({ client: sandbox }),
		},
	],
});

export const registry = setup({ use: { vm } });
registry.start();
