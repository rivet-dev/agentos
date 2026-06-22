import { agentOS, setup } from "@rivet-dev/agentos";
import { createSandboxFs, createSandboxBindings } from "@rivet-dev/agentos-sandbox";
import { SandboxAgent } from "sandbox-agent";
import { docker } from "sandbox-agent/docker";

// Start a sandbox through Sandbox Agent. Any provider works; Docker is used here.
// `SandboxAgent` and the provider helpers come from the `sandbox-agent` package.
const sandbox = await SandboxAgent.start({ sandbox: docker() });

// `createSandboxFs` returns a mount plugin descriptor that projects the sandbox
// filesystem into the VM, and `createSandboxBindings` exposes the sandbox's
// process management as bindings.
const vm = agentOS({
	mounts: [
		{ path: "/workspace/sandbox", plugin: createSandboxFs({ client: sandbox }) },
	],
	bindings: [createSandboxBindings({ client: sandbox })],
});

export const registry = setup({ use: { vm } });

registry.start();
