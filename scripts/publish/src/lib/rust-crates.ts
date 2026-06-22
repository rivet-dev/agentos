import { readFileSync } from "node:fs";
import { join } from "node:path";

// a6-OWNED crates published to crates.io in dependency order. The secure-exec
// runtime crates (bridge/kernel/v8-runtime/execution/sidecar/client) live in
// ../secure-exec and are published by secure-exec, not a6.
export const RUST_CRATE_ORDER = [
	"agentos-protocol",
	"agentos-sidecar",
	"agentos-sidecar-browser",
	"agentos-client",
] as const;

export type PublishableRustCrate = (typeof RUST_CRATE_ORDER)[number];

export const RUST_CRATES = RUST_CRATE_ORDER;

function readPackageName(manifestPath: string): string | undefined {
	const manifest = readFileSync(manifestPath, "utf8");
	const match = manifest.match(/^\s*name\s*=\s*"([^"]+)"/m);
	return match?.[1];
}

function workspaceMembers(repoRoot: string): string[] {
	const manifest = readFileSync(join(repoRoot, "Cargo.toml"), "utf8");
	const match = manifest.match(/\[workspace\][\s\S]*?members\s*=\s*\[([\s\S]*?)\]/);
	if (!match) return [];
	return [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
}

export function discoverRustCrates(repoRoot: string): PublishableRustCrate[] {
	const workspaceCrates = new Set<string>();
	for (const member of workspaceMembers(repoRoot)) {
		const packageName = readPackageName(join(repoRoot, member, "Cargo.toml"));
		if (packageName) {
			workspaceCrates.add(packageName);
		}
	}
	return RUST_CRATE_ORDER.filter((crate) => workspaceCrates.has(crate));
}
