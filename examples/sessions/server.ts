import { agentOS, setup } from "@rivet-dev/agentos";
// In a real app: import pi from "@agentos-software/pi";
import pi from "./software/pi";

const vm = agentOS({ software: [pi] });

export const registry = setup({ use: { vm } });
registry.start();
