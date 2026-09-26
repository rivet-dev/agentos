import { setup } from "rivetkit";
import { vercelWorldActors } from "@rivet-dev/vercel-world/registry";

// The hosted agentOS actor runs as a separate static deployment.

export const registry = setup({
	use: { ...vercelWorldActors },
});
