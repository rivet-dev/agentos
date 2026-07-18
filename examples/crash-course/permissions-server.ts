import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

// Observe native ACP permission requests through the ordinary event hook.
const vm = agentOS({
	software: [pi],
	onSessionEvent: async (_c, sessionId, event) => {
		if (event.type === "permission_request") {
			console.log("Permission requested", sessionId, event.requestId);
		}
	},
});

export const registry = setup({ use: { vm } });
registry.start();
