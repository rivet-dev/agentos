#!/usr/bin/env node
// =============================================================================
// secure-exec dependency manager
// =============================================================================
//
// Single tool to control how this workspace (agent-os / a6) consumes secure-exec.
//
// Two modes:
//   pinned  (default for CI/release) — every secure-exec dependency resolves
//           from its published version. The npm versions live in ONE place (the
//           `catalog:` block in pnpm-workspace.yaml); the crate versions live in
//           the root Cargo.toml `[workspace.dependencies]` block. CI needs no
//           sibling checkout.
//   local   (for hacking on secure-exec) — every swappable dependency is
//           redirected at the sibling ../secure-exec checkout via `link:` (npm)
//           and `path = "../secure-exec/..."` (cargo). Reproduces the classic
//           path-dep dev loop.
//
// Bump the whole workspace to a new secure-exec version with ONE command:
//   node scripts/secure-exec-dep.mjs set-version <version>
//
// Commands:
//   node scripts/secure-exec-dep.mjs pinned
//   node scripts/secure-exec-dep.mjs local
//   node scripts/secure-exec-dep.mjs set-version <version>
//   node scripts/secure-exec-dep.mjs status
//
// After switching modes or versions, run `pnpm install` (and a cargo build) so
// the lockfiles pick up the new resolution.
// =============================================================================

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SECURE_EXEC_REL = "../secure-exec"; // sibling checkout, per CLAUDE.md

// Swappable @secure-exec/* packages -> their path under the secure-exec repo.
const SWAPPABLE_SCOPED = {
	"@secure-exec/core": "packages/core",
	"@secure-exec/s3": "registry/file-system/s3",
	"@secure-exec/google-drive": "registry/file-system/google-drive",
	"@secure-exec/sandbox": "registry/tool/sandbox",
};
// @agent-os-pkgs/<name> always maps to registry/software/<name> (renamed 3rd-party pkgs).
const softwareSubpath = (name) => `registry/software/${name.split("/")[1]}`;
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
	"@secure-exec/nodejs": "0.2.1",
	"@secure-exec/s3": "0.2.0-rc.3",
	"@secure-exec/google-drive": "0.2.0-rc.3",
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
	for (const group of ["packages", "examples", "registry/agent"]) {
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
		name.startsWith("@agent-os-pkgs/") ||
		name in SWAPPABLE_SCOPED ||
		REGISTRY_ONLY.has(name)
	);
}
function isSwappable(name) {
	return name.startsWith("@agent-os-pkgs/") || name in SWAPPABLE_SCOPED;
}
function localSubpath(name) {
	if (name.startsWith("@agent-os-pkgs/")) return softwareSubpath(name);
	return SWAPPABLE_SCOPED[name];
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
	const depRe = /"(@(?:secure-exec|agent-os-pkgs)\/[^"]+)"\s*:/g;
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
function rewriteConsumers(mode) {
	let changed = 0;
	for (const m of consumerManifests()) {
		let text = readFileSync(m, "utf8");
		const before = text;
		for (const name of collectNamesIn(text)) {
			const value =
				mode === "local" && isSwappable(name)
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
	const depRe = /"(@(?:secure-exec|agent-os-pkgs)\/[^"]+)"\s*:/g;
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
	if (name.startsWith("@agent-os-pkgs/")) return SEED_SOFTWARE_VERSION;
	return SEED_VERSIONS[name] ?? SEED_SOFTWARE_VERSION;
}
// Which managed group a catalog package belongs to. secure-exec (the runtime)
// and the @agent-os-pkgs/* software packages publish on independent cadences, so
// versions are set per scope.
//   "secure-exec"   -> @secure-exec/* swappable scope (core, s3, google-drive, sandbox)
//   "agent-os-pkgs" -> @agent-os-pkgs/* renamed third-party software packages
//   "registry-only" -> published-only deps pinned independently (e.g. @secure-exec/nodejs)
function catalogScope(name) {
	if (REGISTRY_ONLY.has(name)) return "registry-only";
	if (name.startsWith("@agent-os-pkgs/")) return "agent-os-pkgs";
	return "secure-exec";
}

// scope: undefined => every managed group except registry-only; "secure-exec" or
// "agent-os-pkgs" => only that group is bumped, the others keep their existing pins.
function writeCatalog(setVersion, scope) {
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
		const v = setVersion && inScope ? setVersion : versionFor(name, existing);
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
// commands
// ---------------------------------------------------------------------------
function npmMode() {
	const root = readFileSync(path.join(ROOT, "package.json"), "utf8");
	return /"@(?:secure-exec|agent-os-pkgs)\/[^"]+"\s*:\s*"link:/.test(root)
		? "local"
		: "pinned";
}
function cargoMode() {
	const cargo = readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
	return /path\s*=\s*"\.\.\/secure-exec\/crates\//.test(cargo) ? "local" : "pinned";
}
function currentMode() {
	return npmMode() === cargoMode() ? npmMode() : `hybrid(npm=${npmMode()},cargo=${cargoMode()})`;
}

// scope: undefined = both, "npm" = only package.json/catalog, "cargo" = only Cargo.toml
function apply(mode, setVersion, scope) {
	let npm = 0;
	if (scope !== "cargo") {
		npm = rewriteConsumers(mode);
		writeCatalog(setVersion);
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
	case "set-version": {
		if (!arg) {
			console.error("usage: set-version <version>");
			process.exit(1);
		}
		// Bump EVERY managed npm package (both scopes) to one version. Only correct
		// when secure-exec and the software packages publish at the same version;
		// otherwise use the scoped commands below.
		writeCatalog(arg);
		console.log(`all secure-exec + agent-os-pkgs npm versions pinned to ${arg} (catalog).`);
		console.log("Run: pnpm install to refresh the lockfile.");
		break;
	}
	case "set-secure-exec-version": {
		if (!arg) {
			console.error("usage: set-secure-exec-version <version>");
			process.exit(1);
		}
		// Bump only the @secure-exec/* runtime scope (core, s3, google-drive,
		// sandbox). The cargo crate version is independent — manage it with
		// `set-crate-version` when the sibling crates rebase.
		writeCatalog(arg, "secure-exec");
		console.log(`@secure-exec/* npm versions pinned to ${arg} (catalog).`);
		console.log("Run: pnpm install to refresh the lockfile.");
		break;
	}
	case "set-agent-os-pkgs-version": {
		if (!arg) {
			console.error("usage: set-agent-os-pkgs-version <version>");
			process.exit(1);
		}
		// Bump only the @agent-os-pkgs/* software packages.
		writeCatalog(arg, "agent-os-pkgs");
		console.log(`@agent-os-pkgs/* npm versions pinned to ${arg} (catalog).`);
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
	case "status": {
		const versions = readVersions();
		console.log(`mode: ${currentMode()}`);
		console.log("pinned versions:");
		for (const [n, v] of Object.entries(versions)) console.log(`  ${n}: ${v}`);
		break;
	}
	default:
		console.error(
			"usage: secure-exec-dep.mjs <pinned|local|status|set-version <v>|set-secure-exec-version <v>|set-agent-os-pkgs-version <v>|set-crate-version <v>>",
		);
		process.exit(1);
}
