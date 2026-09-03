/**
 * agentOS package model (Linux-exact, Homebrew-modeled) — client-facing surface.
 *
 * A package is a self-contained directory produced by `@rivet-dev/agentos-toolchain
 * pack`. The HOST no longer projects it: the client forwards only the package
 * directory over the wire and the agentos sidecar owns the `/opt/agentos`
 * projection. Package metadata lives in `<dir>/agentos-package.json`.
 *
 * This module is therefore only the client-facing package-dir surface plus the
 * `/opt/agentos` path constants used for package wiring.
 *
 * See `website/src/content/docs/docs/architecture/packages-and-command-resolution.mdx`.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
export interface PackageProvidesFileDescriptor {
	source: string;
	target: string;
}

export interface PackageProvidesDescriptor {
	env?: Record<string, string>;
	files?: PackageProvidesFileDescriptor[];
}

export interface AgentosPackageManifest {
	name: string;
	version: string;
	provides?: PackageProvidesDescriptor;
}

/** Root of the agentOS package tree inside the VM. */
export const OPT_AGENTOS_ROOT = "/opt/agentos";
/** The symlink farm on `$PATH` (commands link here). */
export const OPT_AGENTOS_BIN = "/opt/agentos/bin";

export type PackageRef = string;
export type SoftwarePackageRef = { packagePath: string };
/** Portable descriptor used to link a package into a running VM. */
export interface PackageDescriptor {
	path: string;
}

/** Discriminate the path-only package reference. */
export function isPackageDescriptor(
	value: unknown,
): value is PackageDescriptor {
	return (
		typeof value === "object" &&
		value !== null &&
		!Array.isArray(value) &&
		typeof (value as { path?: unknown }).path === "string"
	);
}

export function readAgentosPackageManifest(
	dir: string,
): AgentosPackageManifest {
	const manifestPath = join(dir, "agentos-package.json");
	let parsed: unknown;
	try {
		parsed = JSON.parse(readFileSync(manifestPath, "utf8"));
	} catch (error) {
		const wrapped = new Error(
			`Failed to read agentOS package manifest at ${manifestPath}: ${error instanceof Error ? error.message : String(error)}`,
		);
		(wrapped as NodeJS.ErrnoException).code = (
			error as NodeJS.ErrnoException
		).code;
		throw wrapped;
	}
	return validateAgentosPackageManifest(parsed, manifestPath);
}

export function tryReadAgentosPackageManifest(
	dir: string,
): AgentosPackageManifest | undefined {
	try {
		return readAgentosPackageManifest(dir);
	} catch (error) {
		if (
			error instanceof Error &&
			(error as NodeJS.ErrnoException).code === "ENOENT"
		) {
			return undefined;
		}
		throw error;
	}
}

function validateAgentosPackageManifest(
	value: unknown,
	source: string,
): AgentosPackageManifest {
	if (!isPlainObject(value) || typeof value.name !== "string") {
		throw new Error(
			`Invalid agentOS package manifest at ${source}: missing name`,
		);
	}
	if (typeof value.version !== "string") {
		throw new Error(
			`Invalid agentOS package manifest at ${source}: missing version`,
		);
	}
	const manifest: AgentosPackageManifest = {
		name: value.name,
		version: value.version,
	};
	if (value.provides !== undefined) {
		manifest.provides = value.provides as AgentosPackageManifest["provides"];
	}
	return manifest;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}
