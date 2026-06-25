/**
 * Local stand-in for the `@agentos-software/opencode` agent package.
 *
 * In a real app you would `import opencode from "@agentos-software/opencode"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field.
 */
import type { SoftwareInput } from "@rivet-dev/agentos-core";

const opencode = {
	name: "opencode",
	type: "agent",
	packageDir: new URL("..", import.meta.url).pathname,
	requires: ["@agentos-software/opencode"],
	agent: {
		id: "opencode",
		acpAdapter: "@agentos-software/opencode",
		agentPackage: "@agentos-software/opencode",
	},
} satisfies SoftwareInput;

export default opencode;
