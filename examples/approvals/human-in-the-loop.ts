import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	software: [pi],
	// This generic hook observes the same durable event union as clients. A
	// connected client answers permission_request variants with respondPermission.
	onSessionEvent: async (_c, sessionId, event) => {
		if (event.type === "permission_request") {
			console.log("permission requested:", sessionId, event.requestId);
		}
	},
});

export const registry = setup({ use: { vm } });
registry.start();
