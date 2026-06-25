/**
 * Local stand-in for the `@agentos-software/opencode` agent package.
 *
 * In a real app you would `import opencode from "@agentos-software/opencode"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field.
 */
const opencode = {
	name: "opencode",
	agentType: "opencode",
} as const;

export default opencode;
