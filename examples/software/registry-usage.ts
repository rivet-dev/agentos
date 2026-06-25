import { agentOS, setup } from "@rivet-dev/agentos";
import coreutils from "@agentos-software/coreutils";
import ripgrep from "@agentos-software/ripgrep";

const vm = agentOS({ software: [coreutils, ripgrep] });

export const registry = setup({ use: { vm } });
registry.start();
