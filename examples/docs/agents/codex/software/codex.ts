/**
 * Local stand-in for the `@agentos-software/codex` agent package.
 *
 * In a real app you would `import codex from "@agentos-software/codex"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field.
 */
const codex = {
	name: "codex",
	agentType: "codex",
} as const;

export default codex;
