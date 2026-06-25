import { agentOS, setup } from "@rivet-dev/agentos";
import claude from "./software/claude";

const vm = agentOS({ software: [claude] });

export const registry = setup({ use: { vm } });
registry.start();
