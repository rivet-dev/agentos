import { readFileSync } from "node:fs";
import type { SoftwarePackageRef } from "./agentos-package.js";

/**
 * Default software for a bare `AgentOs.create()`. The build stages the list
 * from `software/default-software.json` and its immutable `.aospkg` files into
 * the Core package; runtime resolution never consults npm or scans
 * node_modules. Opt out with `defaultSoftware: false`; add more trusted paths
 * via `software`.
 */
export function resolveDefaultSoftware(): SoftwarePackageRef[] {
	// Published consumers execute this module from dist/, while Vitest executes
	// the TypeScript source directly. The build stages the same immutable
	// artifacts in dist/default-software for both cases.
	const moduleDirectory = new URL(".", import.meta.url);
	const artifactDirectory = moduleDirectory.pathname.endsWith("/src/")
		? new URL("../dist/default-software/", moduleDirectory)
		: new URL("./default-software/", moduleDirectory);
	const names: string[] = JSON.parse(
		readFileSync(new URL("default-software.json", artifactDirectory), "utf8"),
	);

	return names.map((name) => ({
		packagePath: new URL(`${name}.aospkg`, artifactDirectory).pathname,
	}));
}
