import { agentOS, setup } from "@rivet-dev/agentos";
import opencode from "@agentos-software/opencode";

const vm = agentOS({ software: [opencode] });

export const registry = setup({ use: { vm } });
registry.start();
