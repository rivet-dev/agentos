import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
  software: [],
  // Ports exempt from SSRF checks (reachable beyond loopback)
  loopbackExemptPorts: [3000],
});

export const registry = setup({ use: { vm } });
registry.start();
