/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it provides an equivalently-shaped default export to
 * exercise the `software: [...]` config field.
 */
import type { SoftwareInput } from "@rivet-dev/agentos-core";

const pi = {
	name: "pi",
	type: "agent",
	packageDir: import.meta.dirname,
	requires: ["@mariozechner/pi-coding-agent", "pi-acp"],
	agent: {
		id: "pi",
		acpAdapter: "pi-acp",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
} satisfies SoftwareInput;

export default pi;
