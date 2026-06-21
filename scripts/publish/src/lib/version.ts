/**
 * Version management split across two surfaces:
 *
 * - `bumpPackageJsons` — rewrites every discovered publishable package.json
 *   `version` field and injects `optionalDependencies` on meta packages.
 *   Safe to call in CI on every run. Uses discovery as the source of truth.
 *   Does NOT touch Cargo.toml or non-discovered files.
 *
 * - `bumpCargoVersions` — rewrites the Rust `[workspace.package]` version and
 *   the lock-step internal crate `version` fields under `[workspace.dependencies]`
 *   so the crates.io publish chain stays consistent.
 *
 * - `updateSourceFiles` — rewrites non-package.json, non-Cargo source files
 *   (example dependency specs). Called only by the local `cut-release.ts`.
 *
 * - `resolveVersion` / `shouldTagAsLatest` — semver helpers for the local cut.
 */
import * as fs from "node:fs/promises";
import { join, resolve as resolvePath } from "node:path";
import { $ } from "execa";
import { glob } from "glob";
import * as semver from "semver";
import { scoped } from "./logger.js";
import { buildMetaPlatformMap, discoverPackages } from "./packages.js";

const log = scoped("version");

interface PackageJson {
	name?: string;
	version?: string;
	dependencies?: Record<string, string>;
	devDependencies?: Record<string, string>;
	peerDependencies?: Record<string, string>;
	optionalDependencies?: Record<string, string>;
}

const DEP_FIELDS = [
	"dependencies",
	"devDependencies",
	"peerDependencies",
	"optionalDependencies",
] as const;

/**
 * Parse the default `catalog:` block from pnpm-workspace.yaml into a
 * name -> version map. Lightweight line parser (no YAML dep): reads entries
 * under a top-level `catalog:` key until the next non-indented key.
 */
async function readPnpmCatalog(repoRoot: string): Promise<Map<string, string>> {
	const map = new Map<string, string>();
	let text: string;
	try {
		text = await fs.readFile(join(repoRoot, "pnpm-workspace.yaml"), "utf8");
	} catch {
		return map;
	}
	const lines = text.split("\n");
	let inCatalog = false;
	for (const line of lines) {
		if (/^catalog:\s*$/.test(line)) {
			inCatalog = true;
			continue;
		}
		if (inCatalog) {
			// A non-indented, non-comment, non-blank line ends the block.
			if (/^\S/.test(line) && !line.startsWith("#")) break;
			const m = line.match(/^\s+'?([^':\s]+)'?\s*:\s*'?([^'\s#]+)'?/);
			if (m) map.set(m[1], m[2]);
		}
	}
	return map;
}

export interface BumpOptions {
	/** If true, report actions but do not write. */
	dryRun?: boolean;
	/**
	 * When true, only rewrite the `version` field. Does not touch dependency
	 * references or inject `optionalDependencies`. Safe to commit to git
	 * because it preserves `workspace:*` dep specs that the lockfile expects.
	 *
	 * When false (default), also rewrites `workspace:*` deps to the literal
	 * version and injects `optionalDependencies` on meta packages. This is
	 * the publish-time mode used by CI — never committed.
	 */
	versionOnly?: boolean;
}

/**
 * Rewrite every discovered package's `version` to the given string.
 *
 * In full mode (default, `versionOnly: false`): also injects
 * `optionalDependencies` on meta packages and rewrites `workspace:*`
 * dependency references to the literal version. This is the publish-time
 * mode used by CI and must NOT be committed — it breaks
 * `pnpm install --frozen-lockfile` because the lockfile expects
 * `workspace:*`, not literal versions.
 *
 * In version-only mode (`versionOnly: true`): only rewrites the `version`
 * field. Safe to commit. Used by `cut-release.ts` so the repo records the
 * new version in package.jsons without breaking the lockfile.
 *
 * Returns the number of files written.
 */
export async function bumpPackageJsons(
	repoRoot: string,
	version: string,
	opts: BumpOptions = {},
): Promise<number> {
	const packages = discoverPackages(repoRoot);
	const packageNames = new Set(packages.map((p) => p.name));
	const metaPlatformMap = buildMetaPlatformMap(packages);
	const versionOnly = opts.versionOnly ?? false;
	// pnpm `catalog:` specs are a workspace-only protocol; `npm publish` does not
	// resolve them, so rewrite them to the literal catalog version pre-publish.
	const catalog = await readPnpmCatalog(repoRoot);

	let updated = 0;
	for (const pkg of packages) {
		const pkgJsonPath = join(pkg.dir, "package.json");
		const raw = await fs.readFile(pkgJsonPath, "utf8");
		const pkgJson: PackageJson = JSON.parse(raw);

		pkgJson.version = version;

		if (!versionOnly) {
			// Inject optionalDependencies on meta packages so end users get the
			// correct platform-specific binary via npm's os/cpu/libc resolution.
			const platformPkgs = metaPlatformMap.get(pkg.name);
			if (platformPkgs && platformPkgs.length > 0) {
				pkgJson.optionalDependencies = pkgJson.optionalDependencies ?? {};
				for (const platPkg of platformPkgs) {
					pkgJson.optionalDependencies[platPkg] = version;
				}
			}

			for (const field of DEP_FIELDS) {
				const deps = pkgJson[field];
				if (!deps) continue;
				for (const [dep, spec] of Object.entries(deps)) {
					if (typeof spec !== "string") continue;
					if (spec === "catalog:" || spec.startsWith("catalog:")) {
						const resolved = catalog.get(dep);
						if (resolved) deps[dep] = resolved;
						continue;
					}
					if (!spec.startsWith("workspace:")) continue;
					const isOurPkg =
						packageNames.has(dep) || dep.startsWith("@rivet-dev/agentos-");
					if (!isOurPkg) continue;
					deps[dep] = version;
				}
			}
		}

		// Tab-indented, trailing newline — matches the repo convention.
		const newContent = `${JSON.stringify(pkgJson, null, "\t")}\n`;
		if (opts.dryRun) {
			log.info(`[dry-run] would update ${pkg.name} -> ${version}`);
		} else {
			await fs.writeFile(pkgJsonPath, newContent);
			log.info(`updated ${pkg.name} -> ${version}`);
		}
		updated++;
	}

	log.info(`total: ${updated} package.json files updated to ${version}`);
	return updated;
}

/**
 * Rewrite the a6 Rust workspace version (`[workspace.package]`). a6's own crates
 * inherit it via `version.workspace = true`.
 *
 * NOTE: the secure-exec crate dependencies in `[workspace.dependencies]` are
 * deliberately NOT rewritten — their `version` requirement tracks the sibling
 * secure-exec crate workspace version (e.g. `0.2.0-rc.3`), which a preview does
 * not bump. Rewriting them to a6's version breaks the cargo path-dep check.
 */
export async function bumpCargoVersions(
	repoRoot: string,
	version: string,
	opts: Pick<BumpOptions, "dryRun"> = {},
): Promise<void> {
	const cargoTomlPath = join(repoRoot, "Cargo.toml");
	const cargoToml = await fs.readFile(cargoTomlPath, "utf8");
	let next = cargoToml.replace(
		/(\[workspace\.package\]\n(?:[^\n]*\n)*?[ \t]*version = )"[^"]+"/,
		`$1"${version}"`,
	);
	// Bump a6-OWNED crate dep requirements (path = "crates/..."). The secure-exec
	// crate deps (path = "../secure-exec/...") are intentionally NOT bumped — they
	// track the sibling crate version.
	next = next.replace(
		/((?:agent-os|secure-exec)-[a-z0-9-]+ = \{ path = "crates\/[^"]+", version = ")[^"]+(" \})/g,
		`$1${version}$2`,
	);

	if (next === cargoToml) {
		log.info(`Cargo.toml Rust versions already set to ${version}`);
		return;
	}

	if (opts.dryRun) {
		log.info(`[dry-run] would update Cargo.toml Rust versions -> ${version}`);
	} else {
		await fs.writeFile(cargoTomlPath, next);
		log.info(`updated Cargo.toml Rust versions -> ${version}`);
	}
}

/**
 * Rewrite non-package.json, non-Cargo source files to the given version.
 * Called only by the local release cutter. Examples that pin `@rivet-dev/agentos-*`
 * to a literal version (rather than `workspace:*`) get updated so released
 * examples carry the new version. `required: false` because a6 examples use
 * `workspace:*` today, so a no-match is expected and not an error.
 */
export async function updateSourceFiles(
	repoRoot: string,
	version: string,
): Promise<void> {
	const findReplace: Array<{
		path: string;
		find: RegExp;
		replace: string;
		required?: boolean;
	}> = [
		{
			path: "examples/**/package.json",
			find: /"(@rivet-dev\/agent-os-[^"]+)": "\^?[0-9]+\.[0-9]+\.[0-9]+(?:-[^"]+)?"/g,
			replace: `"$1": "^${version}"`,
			required: false,
		},
	];

	for (const { path: globPath, find, replace, required = true } of findReplace) {
		const paths = await glob(globPath, {
			cwd: repoRoot,
			ignore: ["**/node_modules/**"],
		});
		if (paths.length === 0) {
			if (required) {
				throw new Error(`no paths matched: ${globPath}`);
			}
			continue;
		}
		for (const fileRelPath of paths) {
			const filePath = resolvePath(repoRoot, fileRelPath);
			const file = await fs.readFile(filePath, "utf-8");

			find.lastIndex = 0;
			const hasMatch = find.test(file);
			if (!hasMatch) continue;

			find.lastIndex = 0;
			const newFile = file.replace(find, replace);
			await fs.writeFile(filePath, newFile);
			log.info(`updated ${fileRelPath}`);
		}
	}
}

// -----------------------------------------------------------------------------
// Local semver helpers — used only by `cut-release.ts`.
// -----------------------------------------------------------------------------

async function getAllGitVersions(): Promise<string[]> {
	try {
		await $`git fetch --tags --force --quiet`;
	} catch {
		throw new Error(
			"could not fetch git tags — refusing to compute latest flag from stale local tags",
		);
	}
	const result = await $`git tag -l v*`;
	const tags = result.stdout.trim().split("\n").filter(Boolean);
	if (tags.length === 0) return [];
	return tags
		.map((tag) => tag.replace(/^v/, ""))
		.filter((v) => semver.valid(v))
		.sort((a, b) => semver.rcompare(a, b));
}

export async function getLatestGitVersion(): Promise<string | null> {
	const versions = await getAllGitVersions();
	const stable = versions.filter((v) => {
		const p = semver.parse(v);
		return p && p.prerelease.length === 0;
	});
	return stable[0] ?? null;
}

export async function listRecentVersions(limit = 10): Promise<string[]> {
	const all = await getAllGitVersions();
	return all.slice(0, limit);
}

/**
 * Auto-detect whether a version should be tagged as `latest`. A version is
 * `latest` only if it has no prerelease identifier AND is greater than any
 * existing stable git tag.
 */
export async function shouldTagAsLatest(version: string): Promise<boolean> {
	const parsed = semver.parse(version);
	if (!parsed) throw new Error(`invalid semantic version: ${version}`);
	if (parsed.prerelease.length > 0) return false;
	const latest = await getLatestGitVersion();
	if (!latest) return true;
	return semver.gt(version, latest);
}

export interface ResolveVersionOpts {
	version?: string;
	major?: boolean;
	minor?: boolean;
	patch?: boolean;
}

export async function resolveVersion(
	opts: ResolveVersionOpts,
): Promise<string> {
	if (opts.version) {
		if (!semver.valid(opts.version)) {
			throw new Error(`invalid semantic version: ${opts.version}`);
		}
		return opts.version;
	}
	if (!opts.major && !opts.minor && !opts.patch) {
		throw new Error("must provide --version, --major, --minor, or --patch");
	}
	const latest = await getLatestGitVersion();
	if (!latest) {
		throw new Error(
			"no existing version tags found — use --version to set an explicit version",
		);
	}
	let next: string | null = null;
	if (opts.major) next = semver.inc(latest, "major");
	else if (opts.minor) next = semver.inc(latest, "minor");
	else if (opts.patch) next = semver.inc(latest, "patch");
	if (!next) throw new Error("failed to compute next version");
	return next;
}
