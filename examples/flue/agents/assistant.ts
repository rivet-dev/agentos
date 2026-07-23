import { type AgentRouteHandler, createAgent } from "@flue/runtime";
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { registry } from "../actors.js";

export const route: AgentRouteHandler = async (_context, next) => next();

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-5",
	instructions:
		"Help the user work in /workspace. Use filesystem and shell tools when asked.",
	sandbox: agentOSSandbox({ actor: "vm", registry }),
}));
