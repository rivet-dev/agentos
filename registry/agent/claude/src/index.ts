import { defineSoftware } from "@rivet-dev/agentos-core";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");

const claude = defineSoftware({
	name: "claude",
	type: "agent" as const,
	packageDir,
	requires: ["@anthropic-ai/claude-agent-sdk"],
	agent: {
		id: "claude",
		acpAdapter: "@rivet-dev/agentos-claude",
		agentPackage: "@anthropic-ai/claude-agent-sdk",
		staticEnv: {
			CLAUDE_AGENT_SDK_CLIENT_APP: "@rivet-dev/agent-os",
			CLAUDE_CODE_SIMPLE: "1",
			CLAUDE_CODE_FORCE_AGENT_OS_RIPGREP: "1",
			CLAUDE_CODE_DEFER_GROWTHBOOK_INIT: "1",
			CLAUDE_CODE_DISABLE_CWD_PERSIST: "1",
			CLAUDE_CODE_DISABLE_DEV_NULL_REDIRECT: "1",
			CLAUDE_CODE_NODE_SHELL_WRAPPER: "1",
			CLAUDE_CODE_DISABLE_STREAM_JSON_HOOK_EVENTS: "1",
			CLAUDE_CODE_SHELL: "/bin/sh",
			CLAUDE_CODE_SKIP_INITIAL_MESSAGES: "1",
			CLAUDE_CODE_SKIP_SANDBOX_INIT: "1",
			CLAUDE_CODE_SIMPLE_SHELL_EXEC: "1",
			CLAUDE_CODE_SWAP_STDIO: "0",
			CLAUDE_CODE_USE_PIPE_OUTPUT: "1",
			DISABLE_TELEMETRY: "1",
			SHELL: "/bin/sh",
			USE_BUILTIN_RIPGREP: "0",
		},
	},
});

export default claude;
