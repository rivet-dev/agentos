import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

// Auto-approve all permissions server-side
const vm = agentOS({
  software: [pi],
  onPermissionRequest: async (sessionId, request) => {
    console.log("Auto-approving", sessionId, request.permissionId);
  },
});

export const registry = setup({ use: { vm } });
registry.start();
