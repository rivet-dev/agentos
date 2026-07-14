/**
 * agentOS package model (Linux-exact, Homebrew-modeled) — client-facing surface.
 *
 * A package is a self-contained directory produced by `@rivet-dev/agentos-toolchain
 * pack`. The HOST no longer projects it: the client forwards only the package
 * directory over the wire and the secure-exec sidecar owns the `/opt/agentos`
 * projection. Package metadata lives in `<dir>/agentos-package.json`.
 *
 * This module is therefore only the client-facing package-dir surface plus the
 * `/opt/agentos` path constants used for agent-config wiring.
 *
 * See `website/src/content/docs/docs/architecture/packages-and-command-resolution.mdx`.
 */

import type {
	PackageAgentDescriptor,
	PackageRef as ManifestPackageRef,
} from "@agentos-software/manifest";

/** Root of the agentOS package tree inside the VM. */
export const OPT_AGENTOS_ROOT = "/opt/agentos";
/** The symlink farm on `$PATH` (commands link here). */
export const OPT_AGENTOS_BIN = "/opt/agentos/bin";

export type AgentBlock = PackageAgentDescriptor;
export type PackageRef = ManifestPackageRef;
export type SoftwarePackageRef = { packagePath: string };
/** @deprecated Package software is now represented by its package directory. */
export type PackageDescriptor = PackageRef;

/** Discriminate the dir-only package reference. */
export function isPackageDescriptor(
	value: unknown,
): value is PackageDescriptor {
	return typeof value === "string";
}
