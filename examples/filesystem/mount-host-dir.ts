import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    {
      path: "/mnt/code",
      plugin: { id: "host_dir", config: { hostPath: "/path/to/repo" } },
      readOnly: true,
    },
  ],
});

export const registry = setup({ use: { vm } });
registry.start();
