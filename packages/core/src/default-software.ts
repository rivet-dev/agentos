import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Default software + on-demand agents — FULLY DYNAMIC, no hardcoded lists.
 *
 * This package's own `@agentos-software/*` runtime dependencies (from
 * `dependencies`, never `devDependencies`) are the entire universe:
 *
 * - `resolveDefaultSoftware()` — the set a bare `AgentOs.create()` projects:
 *   every NON-agent dependency's registry-built descriptor (`{ packageDir }`,
 *   or an ARRAY for meta-packages like `@agentos-software/common`). Agent
 *   packages are deliberately NOT projected by default — each carries a full
 *   node closure (and pi a V8 snapshot bundle), so they enter a VM only when a
 *   session actually needs them.
 * - `resolveDependencyAgents()` — the agent packages, keyed by their manifest
 *   `name` (the `createSession(id)` id). `createSession` links the matching
 *   package into the running VM on first use (`linkSoftware`), which registers
 *   its entrypoint/env from the package's own `agentos-package.json`.
 *
 * Adding a default command set or an available agent = adding a dependency.
 * Resolution THROWS with build instructions instead of silently skipping: a
 * missing artifact means either `pnpm install` was not run or, with the
 * file-linked deps, the sibling secure-exec registry was not built.
 */

/** A registry package descriptor: a package dir, or `{ packageDir }`. */
type SoftwareDescriptor = string | { packageDir: string };

const PACKAGE_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const requireFromHere = createRequire(import.meta.url);

const BUILD_INSTRUCTIONS =
	"With file-linked deps, build the registry in the sibling secure-exec " +
	"checkout: `just registry-build` (see its registry/README.md).";

function dependencyNames(): string[] {
	const manifest = JSON.parse(
		readFileSync(join(PACKAGE_ROOT, "package.json"), "utf8"),
	) as { dependencies?: Record<string, string> };
	return Object.keys(manifest.dependencies ?? {})
		.filter((name) => name.startsWith("@agentos-software/"))
		.sort();
}

function descriptorDir(ref: SoftwareDescriptor): string {
	return typeof ref === "string" ? ref : ref.packageDir;
}

interface PackedManifest {
	name?: string;
	agent?: { acpEntrypoint?: string };
}

function readPackedManifest(dir: string): PackedManifest | undefined {
	const path = join(dir, "agentos-package.json");
	if (!existsSync(path)) return undefined;
	try {
		return JSON.parse(readFileSync(path, "utf8")) as PackedManifest;
	} catch {
		return undefined;
	}
}

function assertBuilt(name: string, dir: string): void {
	if (!existsSync(join(dir, "package.json"))) {
		throw new Error(
			`software package ${name} is not BUILT (missing ${dir}). ${BUILD_INSTRUCTIONS}`,
		);
	}
}

/**
 * The default (non-agent) software set: every `@agentos-software/*`
 * dependency's descriptor(s) whose packed manifest has no `agent` entry.
 * Library deps (e.g. `@agentos-software/manifest`) export no descriptor and
 * are skipped. Opt out with `AgentOs.create({ defaultSoftware: false })`.
 */
export async function resolveDefaultSoftware(): Promise<SoftwareDescriptor[]> {
	const software: SoftwareDescriptor[] = [];
	for (const name of dependencyNames()) {
		let mod: { default?: unknown };
		try {
			mod = (await import(name)) as { default?: unknown };
		} catch (error) {
			throw new Error(
				`software package ${name} could not be imported — run \`pnpm install\` ` +
					`(it is a dependency of this package): ${String(error)}`,
			);
		}
		if (mod.default === undefined) continue; // library dep, not software
		for (const descriptor of [mod.default].flat() as SoftwareDescriptor[]) {
			const dir = descriptorDir(descriptor);
			assertBuilt(name, dir);
			if (readPackedManifest(dir)?.agent) continue; // agents load lazily
			software.push(descriptor);
		}
	}
	return software;
}

export interface DependencyAgent {
	/** The dependency package name (e.g. `@agentos-software/pi`). */
	dependency: string;
	/** The packed runtime dir to `linkSoftware`. */
	packageDir: string;
	/** The agent's ACP entrypoint command name. */
	acpEntrypoint: string;
}

/**
 * Agent packages among the dependencies, keyed by manifest `name` (the
 * `createSession(id)` id). Sync (no module import — the packed manifest is
 * read straight from `<dep>/dist/package/`). Unbuilt agent deps THROW.
 */
export function resolveDependencyAgents(): Map<string, DependencyAgent> {
	const agents = new Map<string, DependencyAgent>();
	for (const name of dependencyNames()) {
		let entry: string;
		try {
			entry = requireFromHere.resolve(name); // -> <pkg>/dist/index.js
		} catch {
			continue; // not installed — surfaced by resolveDefaultSoftware/import paths
		}
		const dir = join(dirname(entry), "package");
		const manifest = readPackedManifest(dir);
		if (!manifest?.agent) continue;
		if (!manifest.name || !manifest.agent.acpEntrypoint) {
			throw new Error(
				`agent package ${name} has an invalid packed manifest (${dir}/agentos-package.json) — rebuild it (\`just registry-build\`)`,
			);
		}
		assertBuilt(name, dir);
		agents.set(manifest.name, {
			dependency: name,
			packageDir: dir,
			acpEntrypoint: manifest.agent.acpEntrypoint,
		});
	}
	return agents;
}
