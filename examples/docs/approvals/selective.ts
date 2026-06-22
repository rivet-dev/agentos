import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

const vm = agentOS({
  software: [pi],
  onPermissionRequest: async (sessionId, request) => {
    // `request.description` and `request.params` carry the raw ACP permission
    // details (the requested tool, paths, etc.). Inspect them to decide which
    // requests to handle server-side and which to forward to clients.
    const description = request.description ?? "";
    if (description.toLowerCase().includes("read")) {
      console.log("read request handled server-side", sessionId, request.permissionId);
    }
  },
});

export const registry = setup({ use: { vm } });
registry.start();
