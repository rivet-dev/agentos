import { agentOS, setup } from "@rivet-dev/agentos";
import myAgent from "./my-agent.ts";

const vm = agentOS({ software: [myAgent] });
// createSession() launches the agent by spawning its acpEntrypoint:
//   const session = await vm.createSession("my-agent");

export const registry = setup({ use: { vm } });
registry.start();
