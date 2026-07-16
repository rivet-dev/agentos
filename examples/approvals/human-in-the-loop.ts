import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	software: [pi],
	// Runs server-side for every permission request, before any client round-trip.
	onPermissionRequest: async (_c, sessionId, request) => {
		console.log("permission requested:", sessionId, request.permissionId);
	},
});

export const registry = setup({ use: { vm } });
registry.start();
