/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it provides an equivalently-shaped agent descriptor to
 * exercise the `software: [...]` config field.
 */
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineSoftware } from "@rivet-dev/agentos";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const pi = defineSoftware({
	name: "pi",
	type: "agent",
	packageDir,
	requires: [
		"@agentclientprotocol/sdk",
		"@agentos-software/pi",
		"@mariozechner/pi-coding-agent",
	],
	agent: {
		id: "pi",
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
});

export default pi;
