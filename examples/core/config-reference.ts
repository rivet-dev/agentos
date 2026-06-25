import { agentOS, nodeModulesMount, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

const vm = agentOS({
  // Filesystems to mount at boot. Use nodeModulesMount() to expose a host
  // node_modules tree at /root/node_modules.
  mounts: [nodeModulesMount("/path/to/project/node_modules")],
  // Software packages to install in the VM (see /docs/software)
  software: [pi],
  // Ports exempt from SSRF checks
  loopbackExemptPorts: [3000],
  // Extra instructions appended to agent system prompts
  additionalInstructions: "Always write tests first.",

  // Preview URL token lifetimes
  preview: {
    defaultExpiresInSeconds: 3600, // 1 hour (default)
    maxExpiresInSeconds: 86400, // 24 hours (default)
  },

  // Lifecycle hooks (see below)
  onSessionEvent: async (sessionId, event) => {
    console.log("Session event:", sessionId, event.method);
  },
  onPermissionRequest: async (sessionId, request) => {
    console.log("Permission request:", sessionId, request.permissionId);
  },
});

export const registry = setup({ use: { vm } });
registry.start();
