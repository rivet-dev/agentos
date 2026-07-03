import { AgentOs } from "@rivet-dev/agentos-core";
import {
	createSandboxBindings,
	createSandboxFs,
} from "@rivet-dev/agentos-sandbox";
import { SandboxAgent } from "sandbox-agent";
import { docker } from "sandbox-agent/docker";

export async function createSandboxVm() {
	const sandbox = await SandboxAgent.start({ sandbox: docker() });
	try {
		const vm = await AgentOs.create({
			mounts: [
				{
					path: "/home/agentos/sandbox",
					plugin: createSandboxFs({ client: sandbox }),
				},
			],
			bindings: [createSandboxBindings({ client: sandbox })],
		});
		return { vm, sandbox };
	} catch (error) {
		await sandbox.dispose();
		throw error;
	}
}

export async function disposeSandboxVm(
	handle: Awaited<ReturnType<typeof createSandboxVm>>,
) {
	await Promise.allSettled([handle.vm.dispose(), handle.sandbox.dispose()]);
}
