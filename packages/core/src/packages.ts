import type { SoftwarePackageRef } from "./agentos-package.js";

// ── Software Descriptor Types ────────────────────────────────────────

/**
 * Input type for the `software` option. Software is a self-contained package
 * directory that the secure-exec sidecar materializes into the `/opt/agentos`
 * projection. Accepts a package-dir ref or an array of refs for meta-packages.
 */
export type SoftwareEntry = SoftwarePackageRef;
export type SoftwareInput = SoftwareEntry | SoftwareEntry[];

// ── defineSoftware ───────────────────────────────────────────────────

/**
 * Define a software descriptor. A type-safe identity function that validates the
 * package-dir reference at compile time. The sidecar materializes the package into
 * the `/opt/agentos` projection.
 */
export function defineSoftware<T extends SoftwareEntry>(desc: T): T {
	return desc;
}
