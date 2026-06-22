import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

const vm = agentOS({
  software: [pi],
  // The onPermissionRequest hook runs server-side for every request before it
  // is forwarded to clients. Use it to inspect requests in fully automated
  // pipelines without a client round-trip.
  onPermissionRequest: async (sessionId, request) => {
    console.log("auto-approving", sessionId, request.permissionId);
  },
});

export const registry = setup({ use: { vm } });
registry.start();
