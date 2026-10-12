import { resolve } from "node:path";

export const REPO_ROOT = resolve(import.meta.dirname, "../../..");

export interface Platform {
	readonly id: string;
	readonly os: string;
	readonly cpu: string;
	/** npm `libc` value. Only set for Linux. */
	readonly libc?: string;
}

/** Platforms whose install size is reported. The first one is gated by budgets. */
export const PLATFORMS: ReadonlyArray<Platform> = [
	{ id: "linux-x64-gnu", os: "linux", cpu: "x64", libc: "glibc" },
	{ id: "linux-arm64-gnu", os: "linux", cpu: "arm64", libc: "glibc" },
	{ id: "darwin-arm64", os: "darwin", cpu: "arm64" },
	{ id: "darwin-x64", os: "darwin", cpu: "x64" },
	{ id: "win32-x64", os: "win32", cpu: "x64" },
];
export const GATED_PLATFORM = PLATFORMS[0];

/**
 * Packages always resolved from the registry's `latest`, on every side. The
 * publish step injects the per-platform sidecar binaries, which a workspace
 * pack can't reproduce. The `native` check measures those binaries.
 */
export const REGISTRY_PINNED = ["@rivet-dev/agentos-sidecar"];

export interface AssetSeed {
	/** Published package whose `latest` tarball carries the built assets. */
	readonly package: string;
	/** Directory inside the tarball holding the assets. */
	readonly from: string;
	/** Where the build expects an asset, relative to the checkout, or undefined to skip it. */
	readonly to: (file: string) => string | undefined;
}

/**
 * Build outputs the packages bundle that are too slow to build per PR: WASM
 * commands and default software. They're copied from the latest published
 * tarballs, so the base and the PR pack the same assets and only code and
 * dependency changes show up.
 */
export const ASSET_SEEDS: ReadonlyArray<AssetSeed> = [
	{
		package: "@rivet-dev/agentos-runtime-core",
		from: "package/commands/",
		to: (file) => `toolchain/target/wasm32-wasip1/release/commands/${file}`,
	},
	{
		package: "@rivet-dev/agentos-core",
		from: "package/dist/default-software/",
		to: (file) => (file.endsWith(".aospkg") ? `software/${file.slice(0, -".aospkg".length)}/dist/package.aospkg` : undefined),
	},
];

/**
 * Fresh install size caps on the gated platform, in bytes: the size measured
 * on 2026-10-20 plus 20% headroom. Each includes the 144 MB sidecar binary.
 */
export const BUDGETS: Record<string, number> = {
	"@rivet-dev/agentos": 550 * 1024 ** 2,
	"@rivet-dev/agentos-sandbox": 600 * 1024 ** 2,
};

export interface Threshold {
	readonly pct: number;
	readonly abs: number;
}

/**
 * A growth is flagged when it crosses both the relative and the absolute
 * threshold, so tiny packages don't alert on a few kilobytes.
 */
export const THRESHOLDS = {
	install: {
		warn: { pct: 5, abs: 1024 ** 2 },
		alert: { pct: 20, abs: 10 * 1024 ** 2 },
	},
	packages: { warn: { pct: 10, abs: 5 }, alert: { pct: 25, abs: 25 } },
	bundle: {
		warn: { pct: 5, abs: 5 * 1024 },
		alert: { pct: 20, abs: 25 * 1024 },
	},
	binary: {
		warn: { pct: 3, abs: 1024 ** 2 },
		alert: { pct: 10, abs: 5 * 1024 ** 2 },
	},
} satisfies Record<string, { warn: Threshold; alert: Threshold }>;

/** Synthetic version for packed workspace packages, so npm can never pick a registry copy instead. */
export const PACK_VERSION = "0.0.0-pipboy";
