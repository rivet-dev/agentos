import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * The agentOS package manifest (`agentos-package.json`) at a package root.
 *
 * `name` / `agent` / `provides` describe the package to the runtime (they are
 * copied into `dist/package/agentos-package.json` by `build`). `commands` /
 * `aliases` / `stubs` describe how `stage` populates `bin/` from a compiled
 * commands directory.
 */
export interface AgentosPackageManifest {
	name?: string;
	agent?: unknown;
	provides?: unknown;
	/** Command binaries copied from the commands dir into `bin/<name>`. */
	commands?: string[];
	/** alias -> command: staged as a copy of an already-staged `bin/<command>`. */
	aliases?: Record<string, string>;
	/** Commands staged as copies of the commands dir's `_stubs` binary. */
	stubs?: string[];
}

export function readManifest(
	packageDir: string,
): AgentosPackageManifest | undefined {
	const path = join(packageDir, "agentos-package.json");
	if (!existsSync(path)) return undefined;
	let parsed: unknown;
	try {
		parsed = JSON.parse(readFileSync(path, "utf8"));
	} catch (error) {
		throw new Error(`${path} is not valid JSON: ${String(error)}`);
	}
	return parsed as AgentosPackageManifest;
}

export function unscopedName(name: string): string {
	return name.replace(/^@[^/]+\//, "");
}
