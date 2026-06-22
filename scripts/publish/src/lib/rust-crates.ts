import { readFileSync } from "node:fs";
import { join } from "node:path";

// agentOS-OWNED crates published to crates.io in dependency order. The
// secure-exec runtime crates (bridge/kernel/v8-runtime/execution/sidecar/client)
// are consumed from crates.io and published by secure-exec, not agentOS. The
// RivetKit native-plugin ABI crate (rivet-actor-plugin-abi) is published by the
// rivet repo. agentos-sidecar-browser is intentionally absent: it depends on the
// unpublished secure-exec-sidecar-browser crate and is excluded from the
// workspace (see Cargo.toml), so it cannot be published until that lands.
export const RUST_CRATE_ORDER = [
	"agentos-protocol",
	"agentos-sidecar",
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
