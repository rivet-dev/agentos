/**
 * Local stand-in for the `@agentos-software/common` software bundle.
 *
 * In a real app you would `import common from "@agentos-software/common"`; this
 * fixture is self-contained, so it provides an illustrative package-dir ref to
 * exercise the `software: [...]` config field.
 */
const common = {
	packagePath: "/opt/example-agentos-software/common",
} as const;

export default common;
