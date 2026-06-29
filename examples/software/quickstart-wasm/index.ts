import { agentOS, setup } from "@rivet-dev/agentos";
import myCmds from "./my-cmds.ts";

// The compiled commands are now on $PATH inside the VM.
const vm = agentOS({ software: [myCmds] });

export const registry = setup({ use: { vm } });
registry.start();
