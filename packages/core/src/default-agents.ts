import { existsSync, readdirSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { AgentConfig } from "./agents.js";
import {
	OPT_AGENTOS_BIN,
	type PackageRef,
	tryReadAgentosPackageManifest,
} from "./agentos-package.js";

/**
 * Built-in agents, shipped PRE-PACKED as `/opt/agentos` packages (no runtime npm
 * dependency on the agent SDKs). The build script `scripts/pack-agents.mjs` packs
 * each `package` with the toolchain into `<core>/agents/<package>/<version>/`;
 * `AgentOs.create` projects whatever is present by default and exposes it as the
 * friendly `id` via `createSession(id)`.
 */
export interface BuiltinAgentSpec {
	/** Friendly `createSession(id)` name. */
	id: string;
	/** Published package packed into the shipped artifact. */
	package: string;
	/**
	 * Additional packages to project alongside the adapter (no agent block) — for
	 * a CLI adapter that spawns a separate CLI binary. e.g. `pi-cli`'s `pi-acp`
	 * adapter spawns the `pi` CLI from `@mariozechner/pi-coding-agent` via
	 * `PI_ACP_PI_COMMAND` (set in `defaultEnv`).
	 */
	extraPackages?: string[];
	/** Static env merged UNDER user env when launching the adapter. */
	defaultEnv?: Record<string, string>;
}

export const BUILTIN_AGENTS: BuiltinAgentSpec[] = [
	{ id: "pi", package: "@agentos-software/pi" },
	{
		id: "claude",
		package: "@agentos-software/claude-code",
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
	{
		id: "opencode",
		package: "@agentos-software/opencode",
		defaultEnv: {
			OPENCODE_DISABLE_CONFIG_DEP_INSTALL: "1",
			OPENCODE_DISABLE_EMBEDDED_WEB_UI: "1",
		},
	},
	{
		// CLI adapter: pi-acp spawns the projected `pi` CLI via PI_ACP_PI_COMMAND.
		id: "pi-cli",
		package: "pi-acp",
		extraPackages: ["@mariozechner/pi-coding-agent"],
		defaultEnv: { PI_ACP_PI_COMMAND: `${OPT_AGENTOS_BIN}/pi` },
	},
];

/** The shipped pre-packed-agents root inside the core package (`<core>/agents`). */
export function defaultAgentsRoot(): string {
	return join(dirname(fileURLToPath(import.meta.url)), "..", "agents");
}

/** Resolve a built-in agent's packed dir under `agentsRoot`, or `undefined`. */
export function resolveBuiltinAgentDir(
	agentsRoot: string,
	pkg: string,
): string | undefined {
	const pkgRoot = join(agentsRoot, pkg);
	if (!existsSync(pkgRoot)) return undefined;
	// Resolve `current` to the REAL version dir — the projection copies the dir's
	// contents and would otherwise turn the `current` symlink into a dangling link.
	const current = join(pkgRoot, "current");
	if (existsSync(current)) return realpathSync(current);
	// Fall back to the lone version dir if `current` is absent.
	const versions = readdirSync(pkgRoot).filter((entry) => entry !== "current");
	return versions.length > 0 ? join(pkgRoot, versions[0]) : undefined;
}

export interface ResolvedBuiltinAgents {
	/** Package dirs for every packed built-in agent present. */
	software: PackageRef[];
	/** Agent configs keyed by friendly id, launching via `/opt/agentos/bin/<acp>`. */
	configs: Map<string, AgentConfig>;
}

/**
 * Build the default agent software + configs from the pre-packed artifacts under
 * `agentsRoot`. Agents whose packed dir is absent are skipped (the build may not
 * have packed them in this environment), so this never throws.
 */
export function resolveBuiltinAgents(agentsRoot: string): ResolvedBuiltinAgents {
	const software: PackageRef[] = [];
	const configs = new Map<string, AgentConfig>();
	for (const agent of BUILTIN_AGENTS) {
		const dir = resolveBuiltinAgentDir(agentsRoot, agent.package);
		if (!dir) continue;
		const manifest = tryReadAgentosPackageManifest(dir);
		if (!manifest?.agent) continue;
		// A CLI adapter needs its extra packages (e.g. the spawned CLI) present too;
		// skip the whole agent if any are missing rather than register a broken one.
		const extraDirs = (agent.extraPackages ?? []).map((pkg) =>
			resolveBuiltinAgentDir(agentsRoot, pkg),
		);
		if (extraDirs.some((entry) => entry === undefined)) continue;
		software.push(dir);
		for (const extra of extraDirs) {
			// biome-ignore lint/style/noNonNullAssertion: guarded by the `some` check above.
			software.push(extra!);
		}
		configs.set(agent.id, {
			adapterEntrypoint: `${OPT_AGENTOS_BIN}/${manifest.agent.acpEntrypoint}`,
			defaultEnv: agent.defaultEnv,
		});
	}
	return { software, configs };
}
