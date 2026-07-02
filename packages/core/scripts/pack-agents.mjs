#!/usr/bin/env node
// Pre-pack the built-in agents into `<core>/agents/` as `/opt/agentos` packages.
// These are shipped artifacts (gitignored, built) so the runtime carries NO npm
// dependency on the agent SDKs — `AgentOs.create` projects them at boot and
// `createSession(id)` launches them via `/opt/agentos/bin/<acpEntrypoint>`.
//
// Usage: node scripts/pack-agents.mjs [--agent <id>]...
import { execFileSync } from "node:child_process";
import {
	existsSync,
	mkdirSync,
	readFileSync,
	renameSync,
	rmSync,
	symlinkSync,
} from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const coreRoot = resolve(here, "..");
const outRoot = join(coreRoot, "agents");
// The toolchain lives in the sibling secure-exec repo (packaging is a secure-exec
// concern — it builds the registry packages secure-exec defines). Local dev keeps
// secure-exec checked out next to this repo; otherwise fall back to the published
// @rivet-dev/agentos-toolchain if it is installed.
function resolveToolchainCli() {
	const sibling = resolve(
		coreRoot,
		"../../../secure-exec/packages/agentos-toolchain/dist/cli.js",
	);
	if (existsSync(sibling)) return sibling;
	try {
		return createRequire(import.meta.url).resolve(
			"@rivet-dev/agentos-toolchain/dist/cli.js",
		);
	} catch {
		return undefined;
	}
}
const toolchainCli = resolveToolchainCli();

// Mirror of BUILTIN_AGENTS in src/default-agents.ts (kept in sync). `extraPackages`
// are projected alongside the adapter (no --agent) for a CLI adapter's CLI binary.
const AGENTS = [
	{ id: "pi", package: "@agentos-software/pi", acpEntrypoint: "pi-sdk-acp" },
	{ id: "claude", package: "@agentos-software/claude-code", acpEntrypoint: "claude-sdk-acp" },
	{ id: "opencode", package: "@agentos-software/opencode", acpEntrypoint: "agentos-opencode-acp" },
	{
		id: "pi-cli",
		package: "pi-acp",
		acpEntrypoint: "pi-acp",
		extraPackages: ["@mariozechner/pi-coding-agent"],
	},
];

if (!toolchainCli || !existsSync(toolchainCli)) {
	// No toolchain available (no sibling secure-exec checkout and the published
	// @rivet-dev/agentos-toolchain is not installed). Built-in agents are optional
	// shipped artifacts — `resolveBuiltinAgents()` skips any whose packed dir is
	// absent — so skip packing rather than failing the build/publish. The runtime
	// simply ships without bundled built-in agents.
	console.warn(
		"[pack-agents] WARNING: no agentos-toolchain found (sibling secure-exec or " +
			"@rivet-dev/agentos-toolchain) — SKIPPING built-in agent packing. This build " +
			"ships WITHOUT bundled agents (pi/claude/opencode/pi-cli).",
	);
	process.exit(0);
}

const argv = process.argv.slice(2);
const only = [];
for (let i = 0; i < argv.length; i++) {
	if (argv[i] === "--agent") only.push(argv[++i]);
}

// The toolchain writes a FLAT package into `--out` ("the package IS out"). The
// runtime (`resolveBuiltinAgentDir`) expects the versioned `<name>/<version>` +
// `current` layout, so pack into a temp dir, read the packed name/version, then
// move it under `<outRoot>/<name>/<version>` and repoint `current`.
function pack(args) {
	const tmp = join(outRoot, ".pack-tmp");
	rmSync(tmp, { recursive: true, force: true });
	mkdirSync(tmp, { recursive: true });
	execFileSync("node", [toolchainCli, "pack", ...args, "--prune-native", "--out", tmp], {
		stdio: "inherit",
	});
	const pkg = JSON.parse(readFileSync(join(tmp, "package.json"), "utf8"));
	const versionDir = join(outRoot, pkg.name, pkg.version);
	rmSync(versionDir, { recursive: true, force: true });
	mkdirSync(dirname(versionDir), { recursive: true });
	renameSync(tmp, versionDir);
	const current = join(outRoot, pkg.name, "current");
	rmSync(current, { recursive: true, force: true });
	symlinkSync(pkg.version, current);
}

const selected = only.length ? AGENTS.filter((a) => only.includes(a.id)) : AGENTS;
for (const agent of selected) {
	console.log(`[pack-agents] packing ${agent.package} (${agent.acpEntrypoint})`);
	pack([agent.package, "--agent", agent.acpEntrypoint]);
	for (const extra of agent.extraPackages ?? []) {
		console.log(`[pack-agents]   + extra package ${extra}`);
		pack([extra]);
	}
}
console.log(`[pack-agents] done → ${outRoot}`);
