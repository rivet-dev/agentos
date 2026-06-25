import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({ software: [pi /*, …more */] });

export const registry = setup({ use: { vm } });
registry.start();
