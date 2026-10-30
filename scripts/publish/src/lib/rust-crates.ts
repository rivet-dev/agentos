import { readFileSync } from "node:fs";
import { join } from "node:path";

// AgentOS-owned crates published to crates.io in dependency order. Crates with
// `publish = false` stay out of this list.
export const RUST_CRATE_ORDER = [
	"agentos-rivetkit-ars-client",
	"agentos-vm-host-interface",
	"agentos-executor-contract",
	"agentos-resource-accounting",
	"agentos-executor-wasm-abi",
	"agentos-driver-tokio",
	"agentos-vfs-core",
	"agentos-vfs-storage",
	"agentos-vm-kernel",
	"agentos-vm-config",
	"agentos-sidecar-protocol",
	"agentos-executor-v8-runtime",
	"agentos-executor-node-v8",
	"agentos-executor-python-v8-pyodide",
	"agentos-executor-wasm-v8",
	"agentos-executor-wasm-wasmtime",
	"agentos-sidecar-client",
	"agentos-vm",
	"agentos-client",
	"agentos-sidecar",
] as const;

export type PublishableRustCrate = (typeof RUST_CRATE_ORDER)[number];

export const RUST_CRATES = RUST_CRATE_ORDER;

// This opt-in executor is kept outside the default Cargo workspace so normal
// workspace checks do not compile it. It still ships before its optional
// dependencies in agentos-vm and agentos-sidecar.
export const STANDALONE_RUST_CRATE_MANIFESTS = {
	"agentos-executor-wasm-wasmtime":
		"crates/executor-wasm-wasmtime/Cargo.toml",
} as const;

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
	for (const [name, relativePath] of Object.entries(
		STANDALONE_RUST_CRATE_MANIFESTS,
	)) {
		try {
			if (readPackageName(join(repoRoot, relativePath)) === name) {
				workspaceCrates.add(name);
			}
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
	}
	return RUST_CRATE_ORDER.filter((crate) => workspaceCrates.has(crate));
}
