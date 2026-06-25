/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it provides an equivalently-shaped default export (an
 * agent `SoftwareDescriptor`) to exercise the `software: [...]` config field.
 */
import { defineSoftware } from "@rivet-dev/agentos";

const pi = defineSoftware({
	name: "pi",
	type: "agent",
	packageDir: new URL(".", import.meta.url).pathname,
	requires: ["@agentos-software/pi"],
	agent: {
		id: "pi",
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
});

export default pi;
