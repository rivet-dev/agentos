import { agentOS, setup } from "@rivet-dev/agentos";
import { createInMemoryFileSystem } from "@rivet-dev/agent-os-core";
import pi from "./software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    { path: "/home/user/scratch", driver: createInMemoryFileSystem() },
  ],
});

export const registry = setup({ use: { vm } });
registry.start();
