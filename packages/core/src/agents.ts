// Agent configurations for ACP-compatible coding agents

export interface AgentConfig {
	/**
	 * npm package name for the ACP adapter (spawned inside the VM). Optional: an
	 * `/opt/agentos` agent package sets `adapterEntrypoint` instead.
	 */
	acpAdapter?: string;
	/** npm package name for the underlying agent (optional for `/opt/agentos` packages). */
	agentPackage?: string;
	/**
	 * Pre-resolved guest command path/name for the ACP adapter (e.g.
	 * `/opt/agentos/bin/<acpEntrypoint>`). When set, it is used directly as the
	 * adapter entrypoint and the npm-package resolution (`acpAdapter` →
	 * `/root/node_modules/...`) is bypassed. Set by `/opt/agentos` agent packages.
	 */
	adapterEntrypoint?: string;
	/**
	 * Absolute host path to the software package directory that registered this
	 * agent config. Package-provided agent adapters should resolve their nested
	 * dependencies relative to this directory before falling back to the host dir
	 * behind the caller-supplied `/root/node_modules` mount.
	 */
	declaringPackageDir?: string;
	/** Additional CLI args prepended when launching the ACP adapter. */
	launchArgs?: string[];
	/**
	 * Default env vars to pass when spawning the adapter. These are merged
	 * UNDER user env (lowest priority).
	 * Typically set by package descriptors for computed paths (e.g. PI_ACP_PI_COMMAND).
	 */
	defaultEnv?: Record<string, string>;
}

/**
 * @deprecated npm-era legacy. Agents are `/opt/agentos` packages resolved from
 * `@agentos-software/*` dependency manifests (see `default-software.ts`); this
 * table is no longer consulted by agent resolution and exists only for the
 * exported `AgentType` union and legacy registry-listing metadata.
 */
export const AGENT_CONFIGS = {
	pi: {
		acpAdapter: "@agentos-software/pi",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
	"pi-cli": {
		acpAdapter: "pi-acp",
		agentPackage: "@mariozechner/pi-coding-agent",
	},
	opencode: {
		acpAdapter: "@agentos-software/opencode",
		agentPackage: "@agentos-software/opencode",
		defaultEnv: {
			OPENCODE_DISABLE_CONFIG_DEP_INSTALL: "1",
			OPENCODE_DISABLE_EMBEDDED_WEB_UI: "1",
		},
	},
	claude: {
		acpAdapter: "@agentos-software/claude-code",
		agentPackage: "@anthropic-ai/claude-agent-sdk",
		defaultEnv: {
			CLAUDE_AGENT_SDK_CLIENT_APP: "@rivet-dev/agentos",
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
} satisfies Record<string, AgentConfig>;

export type AgentType = keyof typeof AGENT_CONFIGS;
