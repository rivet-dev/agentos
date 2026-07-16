import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	software: [pi],
	onPermissionRequest: async (_c, sessionId, request) => {
		// `request.description` and `request.params` carry the raw ACP permission
		// details (the requested tool, paths, etc.). Inspect them to decide which
		// requests to handle server-side and which to forward to clients.
		const description = request.description ?? "";
		if (description.toLowerCase().includes("read")) {
			console.log(
				"read request handled server-side",
				sessionId,
				request.permissionId,
			);
		}
	},
});

export const registry = setup({ use: { vm } });
registry.start();
