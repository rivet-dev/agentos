import commandCode from "@agentos-software/command-code";
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({ software: [commandCode] });

export const registry = setup({ use: { vm } });
registry.start();
