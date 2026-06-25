import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export default defineSoftware({
	name: "pi",
	type: "agent",
	packageDir,
	requires: ["@agentos-software/pi", "@mariozechner/pi-coding-agent"],
	agent: {
		id: "pi",
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
});
