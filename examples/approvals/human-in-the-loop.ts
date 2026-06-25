import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

const vm = agentOS({
  software: [pi],
  // Runs server-side for every permission request, before any client round-trip.
  onPermissionRequest: async (sessionId, request) => {
    console.log("permission requested:", sessionId, request.permissionId);
  },
});

export const registry = setup({ use: { vm } });
registry.start();
