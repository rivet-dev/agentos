import { createAgent } from "@flue/runtime";
import { agentOSSandbox } from "@rivet-dev/agentos-flue";

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-5",
	sandbox: agentOSSandbox(),
}));
