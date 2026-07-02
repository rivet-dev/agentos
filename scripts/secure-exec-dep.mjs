#!/usr/bin/env node
// =============================================================================
// secure-exec dependency manager
// =============================================================================
//
// Single tool to control how this workspace (agent-os / a6) consumes secure-exec.
//
// TWO INDEPENDENT TRACKS, each with a pinned and a local mode:
//   runtime  — the @secure-exec/* npm packages + secure-exec-* crates.
//              `pinned` (default): published versions from the catalog +
//              Cargo.toml [workspace.dependencies]. `local`: link:/path deps
//              at the sibling ../secure-exec checkout.
//   registry — the @agentos-software/* packages (registry software + agent
//              adapters). Pinned PER-PACKAGE in the catalog (they version
//              independently); `agentos-pkgs-local` flips them to link: deps at
//              the sibling checkout's registry/{software,agent}/*.
// Flipping one track never touches the other.
//
// PREVIEW CRATE BUILDS (`prepare-build`):
//   npm has dist-tags, so a secure-exec *preview* publishes `@secure-exec/*` to
//   npm under a branch tag (version `0.0.0-<branch>.<sha>`). crates.io has NO
//   such non-prod track, so secure-exec only publishes CRATES on real rc /
//   releases (its publish workflow skips crates.io for previews). The agent-os
//   sidecar is a Rust binary that embeds the secure-exec crates, so to build it
//   against an unreleased (preview) secure-exec we CLONE secure-exec at the
//   pinned commit (the `<sha>` encoded in the npm preview version) and build
//   cargo in `local` (path-dep) mode against that clone. Release pins resolve
//   crates straight from crates.io and need no clone. CI runs `prepare-build`
//   before every `cargo build`; it is a no-op for release pins.
//
// Commands (runtime track):
//   node scripts/secure-exec-dep.mjs pinned | local
//   node scripts/secure-exec-dep.mjs pin-secure-exec <version>
//   node scripts/secure-exec-dep.mjs set-secure-exec-version <version>
//   node scripts/secure-exec-dep.mjs set-crate-version <version>
//   node scripts/secure-exec-dep.mjs prepare-build   # CI: clone+local for previews, no-op for releases
//   node scripts/secure-exec-dep.mjs secure-exec-sha # print the pinned preview sha ("" for releases)
// Commands (registry track):
//   node scripts/secure-exec-dep.mjs agentos-pkgs-pinned | agentos-pkgs-local
//   node scripts/secure-exec-dep.mjs set-agentos-pkg-version <pkg> <version>   # pin ONE package
//   node scripts/secure-exec-dep.mjs agentos-pkgs-update [dist-tag]            # pin each package from npm (default: latest)
// Both:
//   node scripts/secure-exec-dep.mjs status
//
// After switching modes or versions, run `pnpm install` (and a cargo build) so
// the lockfiles pick up the new resolution.
// =============================================================================

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// Sibling checkout, per CLAUDE.md. Overridable via SECURE_EXEC_LOCAL_PATH (a path
// relative to the repo root) for local-only dev against a non-default secure-exec
// working copy — e.g. the converged browser/wasm branch in ../secure-exec-convwasi.
// NEVER push the resulting local path:/link: deps; this only affects `local` mode.
const SECURE_EXEC_REL = process.env.SECURE_EXEC_LOCAL_PATH ?? "../secure-exec";

// Swappable @secure-exec/* packages -> their path under the secure-exec repo.
const SWAPPABLE_SCOPED = {
	"@secure-exec/core": "packages/core",
	"@secure-exec/browser": "packages/browser",
	"@secure-exec/sandbox": "registry/tool/sandbox",
};
// Agent packages are owned by secure-exec under registry/agent/*; generic VM
// software packages are owned under registry/software/*.
const AGENT_PACKAGE_SUBPATHS = {
	"@agentos-software/claude-code": "registry/agent/claude",
	"@agentos-software/codex": "registry/agent/codex",
	"@agentos-software/opencode": "registry/agent/opencode",
	"@agentos-software/pi": "registry/agent/pi",
	"@agentos-software/pi-cli": "registry/agent/pi-cli",
};
const SOFTWARE_PACKAGE_SUBPATHS = {
	// The manifest is a secure-exec workspace package, not registry software.
	"@agentos-software/manifest": "packages/manifest",
};
const softwareSubpath = (name) =>
	SOFTWARE_PACKAGE_SUBPATHS[name] ?? `registry/software/${name.split("/")[1]}`;
// Published-only deps with no local source: always resolved from the registry.
const REGISTRY_ONLY = new Set(["@secure-exec/nodejs"]);

// Crate (cargo) deps -> path under the secure-exec repo.
const CRATES = {
	"secure-exec-bridge": "crates/bridge",
	"secure-exec-kernel": "crates/kernel",
	"secure-exec-execution": "crates/execution",
	"secure-exec-v8-runtime": "crates/v8-runtime",
	"secure-exec-client": "crates/secure-exec-client",
	"secure-exec-sidecar": "crates/sidecar",
	"secure-exec-sidecar-browser": "crates/sidecar-browser",
	"secure-exec-vm-config": "crates/vm-config",
};

// Seed versions (heterogeneous today; `set-version` unifies them after a publish).
const SEED_VERSIONS = {
	"@secure-exec/core": "0.2.1",
	"@secure-exec/browser": "0.2.1",
	"@secure-exec/nodejs": "0.2.1",
	"@secure-exec/sandbox": "0.2.0-rc.3",
};
const SEED_SOFTWARE_VERSION = "0.0.260331072558";
const SEED_CRATE_VERSION = "0.2.0-rc.3";

const CATALOG_BEGIN = "# >>> secure-exec catalog (managed by scripts/secure-exec-dep.mjs) >>>";
const CATALOG_END = "# <<< secure-exec catalog <<<";

// ---------------------------------------------------------------------------
// consumer discovery
// ---------------------------------------------------------------------------
function consumerManifests() {
	const dirs = [ROOT];
	for (const group of ["packages", "examples"]) {
		const base = path.join(ROOT, group);
		if (!existsSync(base)) continue;
		for (const entry of readdirSync(base, { withFileTypes: true })) {
			if (entry.isDirectory()) dirs.push(path.join(base, entry.name));
		}
	}
	return dirs
		.map((d) => path.join(d, "package.json"))
		.filter((p) => existsSync(p));
}

function isManaged(name) {
	return (
		name.startsWith("@agentos-software/") ||
		name in SWAPPABLE_SCOPED ||
		REGISTRY_ONLY.has(name)
	);
}
function isSwappable(name) {
	return name.startsWith("@agentos-software/") || name in SWAPPABLE_SCOPED;
}
function localSubpath(name) {
	if (name.startsWith("@agentos-software/")) {
		return AGENT_PACKAGE_SUBPATHS[name] ?? softwareSubpath(name);
	}
	return SWAPPABLE_SCOPED[name];
}
// A package is only locally swappable if the sibling checkout actually provides
// it: the mapped dir must exist AND its package.json name must match. This skips
// agent-os-owned adapters (@agentos-software/pi, pi-cli, claude-code, codex,
// opencode) that live in registry/agent here and are absent from secure-exec,
// and avoids the registry/software/codex dir (named @agentos-software/codex-cli)
// being mis-linked for the @agentos-software/codex adapter.
function siblingProvides(name) {
	const sub = localSubpath(name);
	if (!sub) return false;
	const pkgPath = path.join(ROOT, SECURE_EXEC_REL, sub, "package.json");
	if (!existsSync(pkgPath)) return false;
	try {
		return JSON.parse(readFileSync(pkgPath, "utf8")).name === name;
	} catch {
		return false;
	}
}

// Relative `link:` path from a consuming package dir to the secure-exec subdir.
function linkValue(manifestPath, name) {
	const consumerDir = path.dirname(manifestPath);
	const target = path.join(ROOT, SECURE_EXEC_REL, localSubpath(name));
	let rel = path.relative(consumerDir, target);
	if (!rel.startsWith(".")) rel = `./${rel}`;
	return `link:${rel}`;
}

// Collect every managed dep name referenced anywhere (for catalog completeness).
function collectManagedNames() {
	const names = new Set();
	const depRe = /"(@(?:secure-exec|agentos-software)\/[^"]+)"\s*:/g;
	for (const m of consumerManifests()) {
		const text = readFileSync(m, "utf8");
		let g;
		while ((g = depRe.exec(text))) {
			if (isManaged(g[1])) names.add(g[1]);
		}
	}
	return [...names].sort();
}

// ---------------------------------------------------------------------------
// npm: rewrite consumer dep values
// ---------------------------------------------------------------------------
// track: "runtime" (@secure-exec/*) or "registry" (@agentos-software/*). Names
// outside the track are left untouched so the two tracks flip independently.
function trackOf(name) {
	return name.startsWith("@agentos-software/") ? "registry" : "runtime";
}
function rewriteConsumers(mode, track) {
	let changed = 0;
	for (const m of consumerManifests()) {
		let text = readFileSync(m, "utf8");
		const before = text;
		for (const name of collectNamesIn(text)) {
			if (trackOf(name) !== track) continue;
			const value =
				mode === "local" && isSwappable(name) && siblingProvides(name)
					? linkValue(m, name)
					: "catalog:";
			const re = new RegExp(`("${escapeRe(name)}"\\s*:\\s*)"[^"]*"`, "g");
			text = text.replace(re, `$1"${value}"`);
		}
		if (text !== before) {
			writeFileSync(m, text);
			changed++;
		}
	}
	return changed;
}
function collectNamesIn(text) {
	const names = new Set();
	const depRe = /"(@(?:secure-exec|agentos-software)\/[^"]+)"\s*:/g;
	let g;
	while ((g = depRe.exec(text))) if (isManaged(g[1])) names.add(g[1]);
	return names;
}
const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// ---------------------------------------------------------------------------
// pnpm-workspace.yaml catalog block
// ---------------------------------------------------------------------------
function readVersions() {
	// Prefer versions already pinned in the existing catalog block; else seed.
	const wsPath = path.join(ROOT, "pnpm-workspace.yaml");
	const text = readFileSync(wsPath, "utf8");
	const versions = {};
	const block = text.match(
		new RegExp(`${escapeRe(CATALOG_BEGIN)}([\\s\\S]*?)${escapeRe(CATALOG_END)}`),
	);
	if (block) {
		const re = /'([^']+)'\s*:\s*([^\s#]+)/g;
		let g;
		while ((g = re.exec(block[1]))) versions[g[1]] = g[2];
	}
	return versions;
}
function versionFor(name, pinned) {
	if (pinned[name]) return pinned[name];
	if (name.startsWith("@agentos-software/")) return SEED_SOFTWARE_VERSION;
	return SEED_VERSIONS[name] ?? SEED_SOFTWARE_VERSION;
}
// Which managed group a catalog package belongs to. secure-exec (the runtime)
// and the @agentos-software/* registry packages publish on independent cadences, so
// versions are set per scope.
//   "secure-exec"   -> @secure-exec/* swappable scope (core, sandbox)
//   "agentos-pkgs" -> @agentos-software/* registry packages
//   "registry-only" -> published-only deps pinned independently (e.g. @secure-exec/nodejs)
function catalogScope(name) {
	if (REGISTRY_ONLY.has(name)) return "registry-only";
	if (name.startsWith("@agentos-software/")) return "agentos-pkgs";
	return "secure-exec";
}

// scope: undefined => every managed group except registry-only; "secure-exec" or
// "agentos-pkgs" => only that group is bumped, the others keep their existing pins.
function writeCatalog(setVersion, scope, overrides = {}) {
	const wsPath = path.join(ROOT, "pnpm-workspace.yaml");
	let text = readFileSync(wsPath, "utf8");
	const existing = readVersions();
	const names = collectManagedNames();
	const lines = [CATALOG_BEGIN, "catalog:"];
	for (const name of names) {
		const group = catalogScope(name);
		// registry-only packages are never version-managed here; everything else is
		// bumped only when it falls in the targeted scope (no scope = all groups).
		const inScope =
			group !== "registry-only" && (scope === undefined || group === scope);
		const v =
			overrides[name] ??
			(setVersion && inScope ? setVersion : versionFor(name, existing));
		lines.push(`  '${name}': ${v}`);
	}
	lines.push(CATALOG_END);
	const block = lines.join("\n");
	const re = new RegExp(
		`${escapeRe(CATALOG_BEGIN)}[\\s\\S]*?${escapeRe(CATALOG_END)}`,
	);
	if (re.test(text)) {
		text = text.replace(re, block);
	} else {
		text = `${text.replace(/\s*$/, "")}\n\n${block}\n`;
	}
	writeFileSync(wsPath, text);
}

// ---------------------------------------------------------------------------
// Cargo.toml [workspace.dependencies]
// ---------------------------------------------------------------------------
function rewriteCargo(mode, setVersion) {
	const cargoPath = path.join(ROOT, "Cargo.toml");
	const lines = readFileSync(cargoPath, "utf8").split("\n");
	const out = lines.map((line) => {
		const m = line.match(/^(\s*)([A-Za-z0-9_-]+)\s*=\s*\{(.*)\}\s*$/);
		if (!m) return line;
		const [, indent, key, body] = m;
		const pkg = (body.match(/package\s*=\s*"([^"]+)"/) || [])[1];
		const crate = pkg || key;
		if (!(crate in CRATES)) return line;
		const ver =
			setVersion || (body.match(/version\s*=\s*"([^"]+)"/) || [])[1] || SEED_CRATE_VERSION;
		const parts = [];
		if (pkg) parts.push(`package = "${pkg}"`);
		if (mode === "local") parts.push(`path = "${SECURE_EXEC_REL}/${CRATES[crate]}"`);
		parts.push(`version = "${ver}"`);
		return `${indent}${key} = { ${parts.join(", ")} }`;
	});
	writeFileSync(cargoPath, out.join("\n"));
}

// ---------------------------------------------------------------------------
// preview crate builds: clone secure-exec at the pinned sha
// ---------------------------------------------------------------------------
// secure-exec is a PUBLIC GitHub repo, so cloning needs no token. The clone
// target is always the sibling ../secure-exec (SECURE_EXEC_REL) so the cargo
// path deps written by `local` mode resolve unchanged.
const SECURE_EXEC_GIT_URL =
	process.env.SECURE_EXEC_GIT_URL ||
	`https://github.com/${process.env.SECURE_EXEC_REPO || "rivet-dev/secure-exec"}.git`;

// The pinned @secure-exec/core version from the catalog is the source of truth
// for which secure-exec the workspace consumes.
function pinnedSecureExecVersion() {
	const v = readVersions()["@secure-exec/core"];
	if (!v) {
		throw new Error(
			"no @secure-exec/core pin found in the pnpm-workspace.yaml catalog",
		);
	}
	return v;
}

// Preview versions are `0.0.0-<branch>.<sha>` (secure-exec scripts/publish
// lib/context.ts: `${PREVIEW_BASE_VERSION}-${branch}.${GITHUB_SHA.slice(0,7)}`).
// Return the trailing commit sha for previews; null for real releases.
function previewSha(version) {
	const m = /^0\.0\.0-.+\.([0-9a-f]{7,40})$/.exec(version);
	return m ? m[1] : null;
}

function git(args, opts = {}) {
	return execFileSync("git", args, { stdio: "inherit", ...opts });
}

// `git fetch` wants a FULL 40-char sha (GitHub advertises full-sha wants, not
// abbreviations), but the preview version only carries the 7-char short sha.
// Resolve it via the commits API. A token (GITHUB_TOKEN) is optional and only
// raises the anonymous rate limit.
async function resolveFullSha(sha) {
	if (/^[0-9a-f]{40}$/.test(sha)) return sha;
	const repo = process.env.SECURE_EXEC_REPO || "rivet-dev/secure-exec";
	const headers = { "User-Agent": "agentos-secure-exec-dep", Accept: "application/vnd.github+json" };
	const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
	if (token) headers.Authorization = `Bearer ${token}`;
	const res = await fetch(`https://api.github.com/repos/${repo}/commits/${sha}`, { headers });
	if (!res.ok) {
		throw new Error(`could not resolve secure-exec sha ${sha}: GitHub API HTTP ${res.status}`);
	}
	const full = (await res.json()).sha;
	if (!full) throw new Error(`GitHub API returned no sha for ${sha}`);
	return full;
}

async function cloneSecureExecAtSha(sha) {
	const abs = path.resolve(ROOT, SECURE_EXEC_REL);
	const full = await resolveFullSha(sha);
	if (!existsSync(path.join(abs, ".git"))) {
		git(["init", "-q", abs]);
		git(["-C", abs, "remote", "add", "origin", SECURE_EXEC_GIT_URL]);
	}
	git(["-C", abs, "fetch", "--depth", "1", "origin", full]);
	git(["-C", abs, "checkout", "-q", full]);
	return abs;
}

// secure-exec crates share ONE workspace version (kept in sync with npm). The
// crate version is the real semver in source — NOT the `0.0.0-...` npm preview
// string — read from the cloned root Cargo.toml `[workspace.package]`.
function readCloneCrateVersion(dir) {
	const cargo = readFileSync(path.join(dir, "Cargo.toml"), "utf8");
	const m = /\[workspace\.package\][\s\S]*?\bversion\s*=\s*"([^"]+)"/.exec(cargo);
	if (!m) {
		throw new Error(
			`could not read [workspace.package] version from ${dir}/Cargo.toml`,
		);
	}
	return m[1];
}

// ---------------------------------------------------------------------------
// .github/refs/secure-exec — the committed sha this repo develops and CI builds against
// ---------------------------------------------------------------------------
const REF_FILE = path.join(ROOT, ".github", "refs", "secure-exec");

function readRefFile() {
	if (!existsSync(REF_FILE)) {
		throw new Error(".github/refs/secure-exec not found — run `just secure-exec-bump` to pin a secure-exec sha");
	}
	const sha = readFileSync(REF_FILE, "utf8").trim();
	if (!/^[0-9a-f]{40}$/.test(sha)) {
		throw new Error(`.github/refs/secure-exec must hold one full 40-char sha, got "${sha}"`);
	}
	return sha;
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------
function npmMode(track) {
	const re =
		track === "registry"
			? /"@agentos-software\/[^"]+"\s*:\s*"link:/
			: /"@secure-exec\/[^"]+"\s*:\s*"link:/;
	for (const m of consumerManifests()) {
		if (re.test(readFileSync(m, "utf8"))) return "local";
	}
	return "pinned";
}
function cargoMode() {
	const cargo = readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
	// Honor the configured (possibly env-overridden) local path so status reports
	// honestly when pointed at e.g. ../secure-exec-convwasi.
	const rel = SECURE_EXEC_REL.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	return new RegExp(`path\\s*=\\s*"${rel}/crates/`).test(cargo) ? "local" : "pinned";
}
function runtimeMode() {
	const npm = npmMode("runtime");
	return npm === cargoMode() ? npm : `hybrid(npm=${npm},cargo=${cargoMode()})`;
}

// scope: undefined = both, "npm" = only package.json/catalog, "cargo" = only Cargo.toml
function apply(mode, setVersion, scope) {
	let npm = 0;
	if (scope !== "cargo") {
		npm = rewriteConsumers(mode, "runtime");
		writeCatalog(setVersion, "secure-exec");
	}
	if (scope !== "npm") rewriteCargo(mode, setVersion);
	return npm;
}

const [cmd, arg] = process.argv.slice(2);
// Optional scope arg: `pinned npm`, `local cargo`, etc.
const SCOPES = new Set(["npm", "cargo"]);
const scope = SCOPES.has(arg) ? arg : undefined;
switch (cmd) {
	case "pinned": {
		apply("pinned", undefined, scope);
		console.log(`secure-exec deps -> PINNED${scope ? ` (${scope} only)` : ""} (published versions).`);
		console.log("Run: pnpm install   (and a cargo build) to refresh lockfiles.");
		break;
	}
	case "local": {
		apply("local", undefined, scope);
		console.log(`secure-exec deps -> LOCAL${scope ? ` (${scope} only)` : ""} (../secure-exec via link:/path).`);
		console.log("Run: pnpm install   (and a cargo build) to refresh lockfiles.");
		break;
	}
	case "agentos-pkgs-local": {
		const n = rewriteConsumers("local", "registry");
		writeCatalog(undefined, "agentos-pkgs");
		console.log(`@agentos-software/* deps -> LOCAL (${SECURE_EXEC_REL} registry/{software,agent} via link:) in ${n} manifest(s).`);
		console.log("Build them there first (just registry-native + just registry-build), then: pnpm install.");
		break;
	}
	case "agentos-pkgs-pinned": {
		const n = rewriteConsumers("pinned", "registry");
		writeCatalog(undefined, "agentos-pkgs");
		console.log(`@agentos-software/* deps -> PINNED (per-package catalog versions) in ${n} manifest(s).`);
		console.log("Run: pnpm install to refresh the lockfile.");
		break;
	}
	case "set-agentos-pkg-version": {
		const [, pkgArg, verArg] = process.argv.slice(2);
		if (!pkgArg || !verArg) {
			console.error("usage: set-agentos-pkg-version <pkg> <version>   (pkg may omit the @agentos-software/ scope)");
			process.exit(1);
		}
		const name = pkgArg.startsWith("@") ? pkgArg : `@agentos-software/${pkgArg}`;
		if (!collectManagedNames().includes(name)) {
			console.error(`ERROR: ${name} is not referenced by any workspace manifest`);
			process.exit(1);
		}
		writeCatalog(undefined, undefined, { [name]: verArg });
		console.log(`${name} pinned to ${verArg} (catalog).`);
		console.log("Run: pnpm install to refresh the lockfile.");
		break;
	}
	case "agentos-pkgs-update": {
		// Pin every managed @agentos-software/* package to its published version
		// under <dist-tag> (default: latest). Packages without the tag are skipped.
		const tag = arg ?? "latest";
		const overrides = {};
		for (const name of collectManagedNames()) {
			if (trackOf(name) !== "registry") continue;
			try {
				const v = execFileSync("npm", ["view", `${name}@${tag}`, "version"], {
					encoding: "utf8",
					stdio: ["ignore", "pipe", "ignore"],
				}).trim();
				if (v) {
					overrides[name] = v;
					console.log(`  ${name}: ${v}`);
				} else {
					console.log(`  ${name}: SKIP (no ${tag} dist-tag)`);
				}
			} catch {
				console.log(`  ${name}: SKIP (no ${tag} dist-tag)`);
			}
		}
		writeCatalog(undefined, undefined, overrides);
		console.log(`@agentos-software/* pins updated from dist-tag "${tag}" (catalog).`);
		console.log("Run: pnpm install to refresh the lockfile.");
		break;
	}
	case "set-secure-exec-version": {
		if (!arg) {
			console.error("usage: set-secure-exec-version <version>");
			process.exit(1);
		}
		// Bump only the @secure-exec/* runtime scope (core, sandbox). The cargo
		// crate version is independent — manage it with
		// `set-crate-version` when the sibling crates rebase.
		writeCatalog(arg, "secure-exec");
		console.log(`@secure-exec/* npm versions pinned to ${arg} (catalog).`);
		console.log("Run: pnpm install to refresh the lockfile.");
		break;
	}
	case "set-crate-version": {
		if (!arg) {
			console.error("usage: set-crate-version <version>  (must match the sibling crate version)");
			process.exit(1);
		}
		rewriteCargo(cargoMode(), arg);
		console.log(`secure-exec crate version requirement set to ${arg}.`);
		break;
	}
	case "pin-secure-exec": {
		// Pin secure-exec to <v>, correct for BOTH preview and release pins:
		//  - npm @secure-exec/* always pin to <v> (a preview branch tag or a
		//    real release version both resolve from the npm registry).
		//  - crates: npm and crates only share a version on a REAL release (they
		//    publish together). For a preview, <v> is `0.0.0-<branch>.<sha>` which
		//    is NOT a crate version — crates.io has no preview track — so the crate
		//    version requirement is left untouched (stays pinned to the last
		//    crates.io release) and `prepare-build` clones at <sha> to build them.
		if (!arg) {
			console.error("usage: pin-secure-exec <version>");
			process.exit(1);
		}
		writeCatalog(arg, "secure-exec");
		const sha = previewSha(arg);
		if (sha) {
			console.log(
				`@secure-exec/* npm pinned to preview ${arg}; crate version left as-is ` +
					`(crates build from a clone at ${sha} via prepare-build — crates.io has no preview track).`,
			);
		} else {
			rewriteCargo(cargoMode(), arg);
			console.log(`@secure-exec/* npm + secure-exec-* crate version pinned to release ${arg}.`);
		}
		break;
	}
	case "secure-exec-sha": {
		// Print the pinned preview sha (empty for release pins). Lets CI gate
		// steps / cache keys on whether a preview clone is needed.
		console.log(previewSha(pinnedSecureExecVersion()) || "");
		break;
	}
	case "prepare-build": {
		// CI step run before pnpm install / any cargo build. The committed state
		// is FILE-BASED (link:/path at ../secure-exec), so CI must materialize the
		// sibling checkout at the committed .github/refs/secure-exec sha and build what the
		// links resolve to. Idempotent; a warm actions/cache of ../secure-exec
		// keyed on .github/refs/secure-exec makes this near-free.
		//
		// If cargo has been swapped to a pinned release (release-swap during a
		// release publish), crates resolve from crates.io and only the npm side
		// of the sibling is needed for any remaining link: deps.
		if (npmMode("runtime") === "pinned" && npmMode("registry") === "pinned" && cargoMode() === "pinned") {
			// Fully swapped to published versions (release-swap) — nothing resolves
			// through the sibling, so there is nothing to prepare.
			console.log("all deps are pinned to published versions — no sibling needed.");
			break;
		}
		const sha = readRefFile();
		const abs = path.resolve(ROOT, SECURE_EXEC_REL);
		if (!existsSync(path.join(abs, "package.json"))) {
			console.log(`cloning ${SECURE_EXEC_GIT_URL} @ ${sha} -> ${SECURE_EXEC_REL}`);
			await cloneSecureExecAtSha(sha);
		} else {
			console.log(`sibling ${SECURE_EXEC_REL} present — leaving its checkout untouched.`);
		}
		if (process.argv.includes("--clone-only")) {
			// Enough for `pnpm install` in THIS repo (link: only needs the target
			// package.json files); cargo builds need the full prepare.
			console.log("clone-only: sibling materialized, skipping install/build.");
			break;
		}
		// The v8-runtime build.rs needs the secure-exec workspace's node deps
		// (packages/build-tools/node_modules) or it panics — install them.
		console.log("installing secure-exec workspace deps...");
		execFileSync("pnpm", ["install", "--frozen-lockfile"], { cwd: abs, stdio: "inherit" });
		if (process.argv.includes("--build")) {
			// Bootstrap: the registry packages' build scripts invoke the
			// `agentos-toolchain` bin, whose symlinks pnpm only creates once the
			// toolchain's dist exists — build it, then re-install to create them.
			execFileSync("npx", ["turbo", "build", "--filter=@rivet-dev/agentos-toolchain"], {
				cwd: abs,
				stdio: "inherit",
			});
			execFileSync("pnpm", ["install", "--frozen-lockfile"], { cwd: abs, stdio: "inherit" });
			// Native wasm commands (skipped when already built — cache hit).
			const commandsDir = path.join(abs, "registry/native/target/wasm32-wasip1/release/commands");
			if (!existsSync(commandsDir)) {
				console.log("building native wasm commands (cache miss — slow)...");
				execFileSync("make", ["-C", path.join(abs, "registry/native"), "wasm"], { stdio: "inherit" });
			} else {
				console.log("native wasm commands present — skipping make wasm.");
			}
			console.log("building secure-exec TS packages...");
			execFileSync("npx", ["turbo", "build", "--filter=!@secure-exec/website", "--filter=!./examples/*"], {
				cwd: abs,
				stdio: "inherit",
			});
		}
		if (cargoMode() === "local") {
			const crateVer = readCloneCrateVersion(abs);
			rewriteCargo("local", crateVer);
			console.log(`cargo path deps against ${SECURE_EXEC_REL} @ ${sha} (crate version ${crateVer}).`);
		} else {
			console.log("cargo is pinned (release-swap) — crates resolve from crates.io.");
		}
		break;
	}
	case "bump-ref": {
		// Pin .github/refs/secure-exec to <ref> (a sha), or to the sibling checkout's
		// current commit when no arg is given.
		let sha = arg;
		if (!sha) {
			const abs = path.resolve(ROOT, SECURE_EXEC_REL);
			sha = execFileSync("git", ["-C", abs, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
		}
		sha = await resolveFullSha(sha);
		writeFileSync(REF_FILE, `${sha}\n`);
		console.log(`.github/refs/secure-exec -> ${sha}`);
		break;
	}
	case "ref": {
		console.log(readRefFile());
		break;
	}
	case "release-swap": {
		// PUBLISH-ONLY, transient: swap the whole workspace from the committed
		// file deps to published versions so the packed tarballs carry real,
		// resolvable versions. Never commit the result — CI checkouts are
		// ephemeral; local runs revert with `release-revert`.
		//   release-swap <secure-exec-version> [registry-dist-tag]
		const [, secureExecVersion, registryTag] = process.argv.slice(2);
		if (!secureExecVersion) {
			console.error("usage: release-swap <secure-exec-version> [registry-dist-tag]");
			process.exit(1);
		}
		rewriteConsumers("pinned", "runtime");
		rewriteConsumers("pinned", "registry");
		writeCatalog(secureExecVersion, "secure-exec");
		const sha = previewSha(secureExecVersion);
		if (sha) {
			console.log(
				`@secure-exec/* pinned to preview ${secureExecVersion}; crate version left as-is ` +
					`(crates build from the ../secure-exec clone at the committed ref).`,
			);
		} else {
			rewriteCargo("pinned", secureExecVersion);
			console.log(`@secure-exec/* npm + crates pinned to release ${secureExecVersion}.`);
		}
		if (registryTag) {
			const overrides = {};
			for (const name of collectManagedNames()) {
				if (trackOf(name) !== "registry") continue;
				try {
					const v = execFileSync("npm", ["view", `${name}@${registryTag}`, "version"], {
						encoding: "utf8",
						stdio: ["ignore", "pipe", "ignore"],
					}).trim();
					if (v) overrides[name] = v;
					console.log(`  ${name}: ${v || `SKIP (no ${registryTag} dist-tag)`}`);
				} catch {
					console.log(`  ${name}: SKIP (no ${registryTag} dist-tag)`);
				}
			}
			writeCatalog(undefined, undefined, overrides);
		}
		console.log("release-swap complete. Run: pnpm install --no-frozen-lockfile.");
		break;
	}
	case "release-revert": {
		// Undo release-swap on a dev machine: back to the committed file deps.
		rewriteConsumers("local", "runtime");
		rewriteConsumers("local", "registry");
		rewriteCargo("local", undefined);
		console.log("deps reverted to file-based (../secure-exec). Run: pnpm install.");
		break;
	}
	case "status": {
		const versions = readVersions();
		console.log(`runtime  (@secure-exec/* + crates): ${runtimeMode()}`);
		console.log(`registry (@agentos-software/*):     ${npmMode("registry")}`);
		console.log("pinned versions:");
		for (const [n, v] of Object.entries(versions)) console.log(`  ${n}: ${v}`);
		break;
	}
	default:
		console.error(
			"usage: secure-exec-dep.mjs <pinned|local|agentos-pkgs-pinned|agentos-pkgs-local|status|pin-secure-exec <v>|set-secure-exec-version <v>|set-crate-version <v>|set-agentos-pkg-version <pkg> <v>|agentos-pkgs-update [tag]|bump-ref [sha]|ref|release-swap <v> [tag]|release-revert|prepare-build [--clone-only|--build]|secure-exec-sha>",
		);
		process.exit(1);
}
