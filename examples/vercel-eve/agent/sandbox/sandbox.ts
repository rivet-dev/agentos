import { agentOSBackend } from "@rivet-dev/agentos-eve";
import { defineSandbox } from "eve/sandbox";

export default defineSandbox({
	backend: agentOSBackend({ actor: "vm" }),
});
