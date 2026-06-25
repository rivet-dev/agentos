/**
 * Local stand-in for the `@agentos-software/claude-code` agent package.
 *
 * In a real app you would `import claude from "@agentos-software/claude-code"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field.
 */
const claude = {
	name: "claude-code",
	agentType: "claude",
} as const;

export default claude;
