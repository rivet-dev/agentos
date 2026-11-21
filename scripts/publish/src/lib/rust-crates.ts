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
	"agentos-client",
	"agentos-vm",
	"agentos-actor-contract",
	"agentos-preload",
	"agentos-actor",
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

/** Workspace patches do not follow an agentOS crate to its downstream consumers. */
export function assertPublishableV8Source(repoRoot: string): void {
	const lock = readFileSync(join(repoRoot, "Cargo.lock"), "utf8");
	const engines = lock.split(/^\[\[package\]\]$/m).filter((p) => /^name = "v8"$/m.test(p));
	if (engines.length === 0 || engines.some((p) => !/^source = "registry\+https:\/\/github\.com\/rust-lang\/crates\.io-index"$/m.test(p))) {
		throw new Error(
			"Rust publication requires a crates.io V8 dependency that works outside the agentOS workspace. Qualify the registry crate and its matching engine bundle before releasing.",
		);
	}
	const pins = JSON.parse(readFileSync(join(repoRoot, "scripts/ci/v8/archives.json"), "utf8"));
	for (const os of ["unknown-linux-gnu", "apple-darwin"]) {
		for (const arch of ["x86_64", "aarch64"]) {
			const target = `${arch}-${os}`;
			const pin = pins[target];
			if (typeof pin !== "string" || !/^[0-9a-f]{64}$/.test(pin)) {
				throw new Error(`Release requires a durable V8 archive SHA-256 for ${target}.`);
			}
		}
	}
}

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
