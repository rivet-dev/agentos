import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
  software: [],
  preview: {
    defaultExpiresInSeconds: 3600, // 1 hour default
    maxExpiresInSeconds: 86400, // 24 hour maximum
  },
});

export const registry = setup({ use: { vm } });
registry.start();
