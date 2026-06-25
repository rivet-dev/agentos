import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
  mounts: [
    // Host directory (read-only)
    {
      path: "/mnt/code",
      plugin: { id: "host_dir", config: { hostPath: "/path/to/repo" } },
      readOnly: true,
    },
    // S3 bucket
    {
      path: "/mnt/data",
      plugin: { id: "s3", config: { bucket: "my-bucket", prefix: "agent/" } },
    },
  ],
});

export const registry = setup({ use: { vm } });
registry.start();
