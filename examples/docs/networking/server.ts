import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({ software: [] });

export const registry = setup({ use: { vm } });
registry.start();
