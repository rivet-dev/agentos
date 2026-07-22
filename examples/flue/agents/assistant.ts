import { createAgent } from "@flue/runtime";
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { registry } from "../registry.js";

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-4-6",
	sandbox: agentOSSandbox({ actor: "vm", registry }),
}));
