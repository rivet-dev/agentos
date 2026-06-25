/**
 * Local stand-in for the `@agentos-software/common` software bundle.
 *
 * In a real app you would `import common from "@agentos-software/common"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field (typed `unknown[]`,
 * so any imported software package value type-checks).
 */
const common = {
	name: "common",
	commands: ["ls", "cat", "grep", "sed", "awk"],
} as const;

export default common;
