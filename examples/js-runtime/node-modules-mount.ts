import { agentOS, setup, nodeModulesMount } from "@rivet-dev/agentos";

const vm = agentOS({
  // Project a host node_modules tree into the VM (read-only by default).
  mounts: [nodeModulesMount("/absolute/path/to/node_modules")],
});

export const registry = setup({ use: { vm } });
registry.start();
