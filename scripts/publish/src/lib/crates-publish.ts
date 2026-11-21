/**
 * crates.io publish staging and rehearsal.
 *
 * `stageCratesForPublish` is the one copy of what the release job does to the
 * checkout before `cargo publish`. `rehearseCratePublish` runs the same staging
 * at a version that does not exist on crates.io, then packages and verifies
 * every publishable crate from its own tarball and proves that the publish
 * order satisfies every packaged manifest. Nothing is uploaded.
 */
import { execFileSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { $ } from "execa";
import { scoped } from "./logger.js";
import { STANDALONE_RUST_CRATE_MANIFESTS } from "./rust-crates.js";
import { bumpCargoVersions, bumpPackageJsons } from "./version.js";

const log = scoped("crates");

// A version no agentOS crate has on crates.io, so every agentOS dependency
// must resolve from the crates packaged in the same rehearsal.
export const REHEARSAL_VERSION = "0.0.0-rehearsal.0";

// crates.io rejects uploads above this size by default.
const CRATES_IO_MAX_CRATE_BYTES = 10 * 1024 * 1024;

const V8_BRIDGE_OUT_DIR = "crates/executor-v8-runtime/assets/generated";
const BASE_FILESYSTEM = "crates/vfs-core/assets/base-filesystem.json";
const BASE_FILESYSTEM_VENDORED = [
	"crates/vm-kernel/assets/base-filesystem.json",
	"crates/vm/assets/base-filesystem.json",
];

export async function stageCratesForPublish(
	repoRoot: string,
	version: string,
	opts: { repository?: string } = {},
): Promise<void> {
	await bumpPackageJsons(repoRoot, version, {
		versionOnly: true,
		repository: opts.repository,
	});
	await bumpCargoVersions(repoRoot, version);

	// The generated V8 bridge bundle is gitignored, and cargo leaves ignored
	// files out of a package. The published v8-runtime crate panics without it.
	mkdirSync(join(repoRoot, V8_BRIDGE_OUT_DIR), { recursive: true });
	await $({ stdio: "inherit", cwd: repoRoot })`node packages/build-tools/scripts/build-v8-bridge.mjs --out-dir ${V8_BRIDGE_OUT_DIR}`;
	await $({ cwd: repoRoot })`git add -f ${V8_BRIDGE_OUT_DIR}`;

	// The kernel and VM build scripts read the vfs crate's base filesystem in
	// the workspace and fall back to a vendored copy in an isolated package.
	for (const target of BASE_FILESYSTEM_VENDORED) {
		mkdirSync(join(repoRoot, target, ".."), { recursive: true });
		copyFileSync(join(repoRoot, BASE_FILESYSTEM), join(repoRoot, target));
	}
	log.info(`staged crates for publish at ${version}`);
}

interface MetadataDependency {
	name: string;
	kind: "dev" | "build" | null;
	optional: boolean;
	target: string | null;
	req: string;
}

interface MetadataPackage {
	name: string;
	version: string;
	publish: string[] | null;
	manifest_path: string;
	dependencies: MetadataDependency[];
}

function cargoMetadata(manifestPath: string): MetadataPackage[] {
	const output = execFileSync(
		"cargo",
		["metadata", "--no-deps", "--format-version", "1", "--manifest-path", manifestPath],
		{ encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
	);
	return JSON.parse(output).packages;
}

function isPublishable(pkg: MetadataPackage): boolean {
	return pkg.publish === null || pkg.publish.includes("crates-io");
}

function describe(dependency: MetadataDependency): string {
	const parts = [dependency.kind ?? "normal"];
	if (dependency.optional) parts.push("optional");
	if (dependency.target) parts.push(`target ${dependency.target}`);
	return parts.join(", ");
}

/**
 * Reads the normalized `Cargo.toml` inside each `.crate` (what crates.io
 * indexes) and returns one message per problem that would fail a real
 * publish: an agentOS dependency that publishes later or never, a crate
 * missing from or extra to the publish order, or an oversized tarball.
 */
export function findCratePublishOrderProblems(
	repoRoot: string,
	order: readonly string[],
	version: string,
	targetDir: string,
): string[] {
	const problems: string[] = [];
	const workspace = cargoMetadata(join(repoRoot, "Cargo.toml"));
	const publishable = new Set(workspace.filter(isPublishable).map((pkg) => pkg.name));
	const internal = new Set(workspace.map((pkg) => pkg.name));
	for (const name of publishable) {
		if (!order.includes(name)) {
			problems.push(`${name} is publishable but missing from RUST_CRATE_ORDER; the release would skip it`);
		}
	}
	for (const name of order) {
		if (!publishable.has(name)) {
			problems.push(`${name} is in RUST_CRATE_ORDER but is not a publishable package`);
		}
	}

	const scratch = mkdtempSync(join(tmpdir(), "agentos-crate-manifests-"));
	try {
		for (const [index, name] of order.entries()) {
			const crateFile = join(targetDir, "package", `${name}-${version}.crate`);
			let size: number;
			try {
				size = statSync(crateFile).size;
			} catch {
				problems.push(`${name}: ${crateFile} was not produced by cargo package`);
				continue;
			}
			if (size > CRATES_IO_MAX_CRATE_BYTES) {
				problems.push(`${name}: ${crateFile} is ${size} bytes; crates.io rejects crates above ${CRATES_IO_MAX_CRATE_BYTES}`);
			}
			execFileSync("tar", ["-xzf", crateFile, "-C", scratch]);
			const [packaged] = cargoMetadata(join(scratch, `${name}-${version}`, "Cargo.toml"));
			for (const dependency of packaged.dependencies) {
				if (!internal.has(dependency.name)) continue;
				const dependencyIndex = order.indexOf(dependency.name);
				if (dependencyIndex === -1) {
					problems.push(`${name} depends on ${dependency.name} (${describe(dependency)}), which is never published`);
				} else if (dependencyIndex > index) {
					problems.push(
						`${name} depends on ${dependency.name} (${describe(dependency)}), but ${dependency.name} publishes later (position ${dependencyIndex + 1} > ${index + 1} in RUST_CRATE_ORDER); cargo publish -p ${name} cannot resolve it on crates.io`,
					);
				}
			}
		}
	} finally {
		rmSync(scratch, { recursive: true, force: true });
	}
	return problems;
}

/**
 * Adds the standalone crates to the workspace so one `cargo package` overlay
 * holds every publishable crate. In the real publish they come from crates.io.
 * Returns a function that restores the root manifest.
 */
function includeStandaloneCrates(repoRoot: string): () => void {
	const path = join(repoRoot, "Cargo.toml");
	const original = readFileSync(path, "utf8");
	let next = original;
	for (const manifest of Object.values(STANDALONE_RUST_CRATE_MANIFESTS)) {
		const dir = manifest.replace(/\/Cargo\.toml$/, "");
		next = next.replace(new RegExp(`,?\\s*"${dir}"`), "").replace(/members = \[/, `members = [\n    "${dir}",`);
	}
	writeFileSync(path, next);
	return () => writeFileSync(path, original);
}

/**
 * Cargo extracts the local overlay into the registry source cache keyed by
 * name and version and never extracts it again. Without this, a second
 * rehearsal at the same version verifies dependents against the previous
 * run's sources.
 */
function purgeCachedOverlayCrates(order: readonly string[], version: string): void {
	const registry = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "registry");
	for (const [kind, suffix] of [
		["src", ""],
		["cache", ".crate"],
	]) {
		const root = join(registry, kind);
		if (!existsSync(root)) continue;
		for (const index of readdirSync(root)) {
			for (const name of order) {
				rmSync(join(root, index, `${name}-${version}${suffix}`), {
					recursive: true,
					force: true,
				});
			}
		}
	}
}

export async function rehearseCratePublish(
	repoRoot: string,
	order: readonly string[],
	opts: { version?: string; repository?: string; jobs?: number } = {},
): Promise<void> {
	const version = opts.version ?? REHEARSAL_VERSION;
	await stageCratesForPublish(repoRoot, version, { repository: opts.repository });
	const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");
	purgeCachedOverlayCrates(order, version);
	const restore = includeStandaloneCrates(repoRoot);
	try {
		const packages = order.flatMap((name) => ["-p", name]);
		const jobs = opts.jobs ? ["--jobs", String(opts.jobs)] : [];

		log.info(`packaging ${order.length} crates at ${version} to check the publish order`);
		await $({ stdio: "inherit", cwd: repoRoot })`cargo package --allow-dirty --no-verify ${packages}`;
		const problems = findCratePublishOrderProblems(repoRoot, order, version, targetDir);
		if (problems.length > 0) {
			throw new Error(`crates.io publish order is broken:\n  - ${problems.join("\n  - ")}`);
		}
		log.info("publish order satisfies every packaged manifest");

		log.info(`verifying ${order.length} crates from their packaged tarballs`);
		await $({ stdio: "inherit", cwd: repoRoot })`cargo package --allow-dirty ${jobs} ${packages}`;
		log.info(`rehearsal passed: ${order.length} crates package, verify, and publish in order`);
	} finally {
		restore();
	}
}
