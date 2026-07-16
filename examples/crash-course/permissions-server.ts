import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

// Auto-approve all permissions server-side
const vm = agentOS({
	software: [pi],
	onPermissionRequest: async (_c, sessionId, request) => {
		console.log("Auto-approving", sessionId, request.permissionId);
	},
});

export const registry = setup({ use: { vm } });
registry.start();
