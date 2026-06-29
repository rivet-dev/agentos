import { agentOS, setup } from "@rivet-dev/agentos";
import myTool from "./my-tool.ts";

// `my-tool` is now on $PATH inside the VM.
const vm = agentOS({ software: [myTool] });

export const registry = setup({ use: { vm } });
registry.start();
