import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    {
      path: "/home/agentos/scratch",
      plugin: { id: "memory", config: {} },
    },
  ],
});

export const registry = setup({ use: { vm } });
registry.start();
