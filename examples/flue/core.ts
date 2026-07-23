import { createAgent } from "@flue/runtime";
import { AgentOs } from "@rivet-dev/agentos-core";
import { agentOSCoreSandbox } from "@rivet-dev/agentos-flue";

const vms = new Map<string, Promise<AgentOs>>();

function getVm(id: string): Promise<AgentOs> {
	let vm = vms.get(id);
	if (!vm) {
		vm = AgentOs.create({
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
		});
		vms.set(id, vm);
	}
	return vm;
}

// The application owner must call this during shutdown. Flue's sandbox
// interface does not currently expose a disposal hook.
export async function disposeCoreSandboxes(): Promise<void> {
	await Promise.all([...vms.values()].map(async (vm) => (await vm).dispose()));
	vms.clear();
}

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-5",
	sandbox: agentOSCoreSandbox({
		create: ({ id }) => getVm(id),
	}),
}));
