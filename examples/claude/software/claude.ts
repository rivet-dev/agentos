/**
 * Local stand-in for the `@agentos-software/claude-code` agent package.
 *
 * In a real app you would `import claude from "@agentos-software/claude-code"`;
 * this fixture is self-contained, so it provides an equivalently-shaped agent
 * descriptor to exercise the `software: [...]` config field.
 */
import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const claude = defineSoftware({
	name: "claude-code",
	type: "agent",
	packageDir,
	requires: ["@agentos-software/claude-code"],
	agent: {
		// Used in createSession("claude").
		id: "claude",
		acpAdapter: "@agentos-software/claude-code",
		agentPackage: "@anthropic-ai/claude-agent-sdk",
	},
});

export default claude;
