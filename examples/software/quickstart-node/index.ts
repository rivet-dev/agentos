import { AgentOs } from "@rivet-dev/agentos-core";
import myTool from "./my-tool.ts";

// `my-tool` is now on $PATH inside the VM.
export const vm = await AgentOs.create({ software: [myTool] });
