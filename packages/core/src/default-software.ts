import type { SoftwarePackageRef } from "@agentos-software/manifest";
import common from "@agentos-software/common";

/**
 * Default software for a bare `AgentOs.create()`: the `@agentos-software/common`
 * coreutils bundle, imported like any other package. The client makes NO
 * npm/node_modules assumptions (see root CLAUDE.md) — no dep scan, no
 * require.resolve. Opt out with `defaultSoftware: false`; add more via `software`.
 */
export function resolveDefaultSoftware(): SoftwarePackageRef[] {
	return [common].flat() as SoftwarePackageRef[];
}
