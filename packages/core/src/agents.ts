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
 * An agent type id — the `name` of an `/opt/agentos` agent package manifest
 * (e.g. `"pi"`, `"claude"`). Agents are resolved dynamically from the configured
 * package manifests (see `default-software.ts` and `agent-os.ts`), so there is no
 * fixed union; any manifest `name` is a valid agent type.
 */
export type AgentType = string;
