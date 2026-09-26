import { AgentOs } from "@rivet-dev/agentos-core";
import myCmds from "./my-cmds.ts";

// The compiled commands are now on $PATH inside the VM.
export const vm = await AgentOs.create({ software: [myCmds] });
