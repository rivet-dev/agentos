/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it provides an equivalently-shaped default export to
 * exercise the `software: [...]` config field.
 */
const pi = {
	name: "pi",
	type: "agent" as const,
	packageDir: "/example/node_modules/@agentos-software/pi",
	requires: ["@agentos-software/pi", "@mariozechner/pi-coding-agent"],
	agent: {
		id: "pi",
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
};

export default pi;
