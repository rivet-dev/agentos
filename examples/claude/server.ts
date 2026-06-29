import { agentOS, setup } from "@rivet-dev/agentos";
import claude from "@agentos-software/claude-code";

const vm = agentOS({ software: [claude] });

export const registry = setup({ use: { vm } });
registry.start();
