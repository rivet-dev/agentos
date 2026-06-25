import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it defines an equivalently-shaped agent descriptor to
 * exercise the `software: [...]` config field.
 */
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
