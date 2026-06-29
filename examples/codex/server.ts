import { agentOS, setup } from "@rivet-dev/agentos";
import codex from "@agentos-software/codex";

const vm = agentOS({ software: [codex] });

export const registry = setup({ use: { vm } });
registry.start();
