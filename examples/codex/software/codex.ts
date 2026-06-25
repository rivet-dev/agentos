/**
 * Local stand-in for the `@agentos-software/codex` agent package.
 *
 * In a real app you would `import codex from "@agentos-software/codex"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field.
 */
import type { SoftwareInput } from "@rivet-dev/agentos-core";

const codex = {
	name: "codex",
	type: "agent",
	packageDir: new URL("..", import.meta.url).pathname,
	requires: ["@agentos-software/codex"],
	agent: {
		id: "codex",
		acpAdapter: "@agentos-software/codex",
		agentPackage: "@agentos-software/codex",
	},
} satisfies SoftwareInput;

export default codex;
