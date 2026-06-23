import { defineSoftware } from "@rivet-dev/agentos-core";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");

const pi = defineSoftware({
	name: "pi",
	type: "agent" as const,
	packageDir,
	requires: ["@agentos-software/pi", "@mariozechner/pi-coding-agent"],
	agent: {
		id: "pi",
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
});

export default pi;
