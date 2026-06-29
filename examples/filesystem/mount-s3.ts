import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    {
      path: "/mnt/data",
      plugin: {
        id: "s3",
        config: {
          bucket: "my-bucket",
          prefix: "agent-data/",
          region: "us-east-1",
        },
      },
    },
  ],
});

export const registry = setup({ use: { vm } });
registry.start();
