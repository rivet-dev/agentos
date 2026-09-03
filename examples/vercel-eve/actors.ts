import { vercelWorldActors } from "@rivet-dev/vercel-world/registry";
import { setup } from "rivetkit";

export const registry = setup({
	use: vercelWorldActors,
});
