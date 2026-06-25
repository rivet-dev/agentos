/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field.
 */
import type { AgentSoftwareDescriptor } from "@rivet-dev/agentos-core";

const pi: AgentSoftwareDescriptor = {
	name: "pi",
	type: "agent",
	packageDir: "/dev/null",
	requires: ["@agentos-software/pi"],
	agent: {
		id: "pi",
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
};

export default pi;
