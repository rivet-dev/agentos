/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it provides an illustrative package-dir ref to exercise
 * the `software: [...]` config field.
 */
const pi = {
	packagePath: "/opt/example-agentos-software/pi",
} as const;

export default pi;
