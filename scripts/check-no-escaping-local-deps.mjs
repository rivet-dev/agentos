// Guard against committing dependencies that point at a local path *outside*
// this repository, EXCEPT the sibling ../secure-exec checkout. The committed
// dependency state is deliberately file-based: every @secure-exec/* /
// @agentos-software/* npm dep is a link: into ../secure-exec and every
// secure-exec-* crate a path dep there (CI materializes the sibling at the
// committed .github/refs/secure-exec sha via `prepare-build`; publishes swap to real
// versions transiently via `release-swap`). Any OTHER escaping local dep —
// a stray link into some scratch checkout — still fails this check, as does
// an escape pointing anywhere but the sibling secure-exec dir.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dependencySections = [
	"dependencies",
	"devDependencies",
	"peerDependencies",
	"optionalDependencies",
];
// pnpm/npm local-path protocols whose target is a filesystem path.
const localProtocols = ["link:", "file:", "portal:"];
const ignoredDirectories = new Set([
	".git",
	".jj",
	".turbo",
	"coverage",
	"dist",
	"node_modules",
	"target",
]);

function parseArgs(argv) {
	const options = { root: defaultRoot };
	for (let i = 0; i < argv.length; i++) {
		const arg = argv[i];
		if (arg === "--root") {
			options.root = argv[++i];
			continue;
		}
		if (arg.startsWith("--root=")) {
			options.root = arg.slice("--root=".length);
			continue;
		}
		throw new Error(`unknown argument: ${arg}`);
	}
	return { root: resolve(options.root) };
}

// True when `target` is the root itself or nested inside it (lexically — the
// path need not exist, which matters because escaping targets are absent in CI).
function isInsideRoot(root, target) {
	if (target === root) return true;
	const rel = relative(root, target);
	return rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
}

// The one sanctioned escape: the sibling secure-exec checkout (see header).
function isInsideSiblingSecureExec(root, target) {
	const sibling = resolve(root, "../secure-exec");
	return target === sibling || isInsideRoot(sibling, target);
}

function localPathFromSpecifier(specifier) {
	for (const protocol of localProtocols) {
		if (specifier.startsWith(protocol)) {
			return specifier.slice(protocol.length);
		}
	}
	return null;
}

function checkPackageManifest(root, manifestPath, relPath, violations) {
	let manifest;
	try {
		manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
	} catch {
		return;
	}
	const manifestDir = dirname(manifestPath);
	for (const section of dependencySections) {
		const deps = manifest[section];
		if (!deps || typeof deps !== "object") continue;
		for (const [name, specifier] of Object.entries(deps)) {
			if (typeof specifier !== "string") continue;
			const localPath = localPathFromSpecifier(specifier);
			if (localPath === null) continue;
			const resolved = resolve(manifestDir, localPath);
			if (!isInsideRoot(root, resolved) && !isInsideSiblingSecureExec(root, resolved)) {
				violations.push(
					`${relPath} ${section}."${name}" uses local dep "${specifier}" that escapes the repo (and is not the sibling ../secure-exec)`,
				);
			}
		}
	}
}

// Match `path = "..."` entries in a Cargo.toml (deps + non-dep keys alike;
// in-repo paths pass the escape check, so only escaping ones are flagged).
const cargoPathPattern = /(^|[\s{,])path\s*=\s*"([^"]+)"/g;

function checkCargoManifest(root, manifestPath, relPath, violations) {
	const source = readFileSync(manifestPath, "utf8");
	const manifestDir = dirname(manifestPath);
	cargoPathPattern.lastIndex = 0;
	let match;
	while ((match = cargoPathPattern.exec(source))) {
		const localPath = match[2];
		const resolved = resolve(manifestDir, localPath);
		if (!isInsideRoot(root, resolved) && !isInsideSiblingSecureExec(root, resolved)) {
			violations.push(
				`${relPath} uses cargo path = "${localPath}" that escapes the repo (and is not the sibling ../secure-exec)`,
			);
		}
	}
}

function walk(root, dir, violations) {
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (entry.isDirectory() && ignoredDirectories.has(entry.name)) continue;
		const path = resolve(dir, entry.name);
		if (entry.isDirectory()) {
			walk(root, path, violations);
			continue;
		}
		if (!entry.isFile()) continue;
		const relPath = relative(root, path).split(sep).join("/");
		if (entry.name === "package.json") {
			checkPackageManifest(root, path, relPath, violations);
		} else if (entry.name === "Cargo.toml") {
			checkCargoManifest(root, path, relPath, violations);
		}
	}
}

export function auditLocalDeps(options = {}) {
	const root = resolve(options.root ?? defaultRoot);
	const violations = [];
	if (!existsSync(root)) {
		return { root, ok: false, violations: [`${root} does not exist`] };
	}
	walk(root, root, violations);
	violations.sort();
	return { root, ok: violations.length === 0, violations };
}

export function main(argv = process.argv.slice(2)) {
	const options = parseArgs(argv);
	const result = auditLocalDeps(options);
	if (result.ok) {
		console.log("no escaping local deps");
		return 0;
	}
	console.error("escaping local dependency violations:");
	for (const violation of result.violations) {
		console.error(`- ${violation}`);
	}
	console.error(
		"\nCommit pinned/published versions instead of link:/file:/path: deps that point outside the repo.",
	);
	return 1;
}

if (import.meta.url === `file://${process.argv[1]}`) {
	process.exitCode = main();
}
