import type { SoftwarePackageRef } from "./agentos-package.js";

const DEFAULT_SOFTWARE = [
	"coreutils",
	"sed",
	"grep",
	"gawk",
	"findutils",
	"diffutils",
	"tar",
	"gzip",
] as const;

/**
 * Default software for a bare `AgentOs.create()`. These immutable `.aospkg`
 * files are vendored into the Core package at build time; runtime resolution
 * never consults npm or scans node_modules. Opt out with
 * `defaultSoftware: false`; add more trusted paths via `software`.
 */
export function resolveDefaultSoftware(): SoftwarePackageRef[] {
	// Published consumers execute this module from dist/, while Vitest executes
	// the TypeScript source directly. The build stages the same immutable
	// artifacts in dist/default-software for both cases.
	const moduleDirectory = new URL(".", import.meta.url);
	const artifactDirectory = moduleDirectory.pathname.endsWith("/src/")
		? new URL("../dist/default-software/", moduleDirectory)
		: new URL("./default-software/", moduleDirectory);

	return DEFAULT_SOFTWARE.map((name) => ({
		packagePath: new URL(`${name}.aospkg`, artifactDirectory).pathname,
	}));
}
