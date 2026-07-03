import { agentOS } from "@rivet-dev/agentos";
import { docker } from "@rivet-dev/agentos-sandbox";

const vm = agentOS({
	sandbox: {
		provider: docker(),
	},
});
