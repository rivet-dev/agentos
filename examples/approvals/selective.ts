import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	software: [pi],
	onSessionEvent: async (_c, sessionId, event) => {
		if (event.type === "permission_request") {
			console.log(
				"permission request audited server-side",
				sessionId,
				event.requestId,
			);
		}
	},
});

export const registry = setup({ use: { vm } });
registry.start();
