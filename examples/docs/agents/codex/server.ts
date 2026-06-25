import { agentOS, setup } from "@rivet-dev/agentos";
import codex from "./software/codex";

const vm = agentOS({ software: [codex] });

export const registry = setup({ use: { vm } });
registry.start();
