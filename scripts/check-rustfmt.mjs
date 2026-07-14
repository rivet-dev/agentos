import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const disabledBrowserPackages = new Set([
	"agentos-sidecar-browser",
	"agentos-native-sidecar-browser",
]);

function readCargoMetadata(root) {
	const stdout = execFileSync(
		"cargo",
		["metadata", "--format-version", "1", "--no-deps"],
		{
			cwd: root,
			encoding: "utf8",
		},
	);
	return JSON.parse(stdout);
}

/**
 * Resolve Cargo's default workspace selection into package names. Browser
 * sources remain workspace members for explicit maintenance, but must not
 * enter the native formatting gate while browser support is disabled.
 */
export function defaultRustfmtPackages(metadata) {
	if (!Array.isArray(metadata.workspace_default_members)) {
		throw new Error("Cargo metadata is missing workspace_default_members");
	}
	if (!Array.isArray(metadata.packages)) {
		throw new Error("Cargo metadata is missing packages");
	}

	const packagesById = new Map(
		metadata.packages.map((pkg) => [pkg.id, pkg.name]),
	);
	const names = metadata.workspace_default_members.map((id) => {
		const name = packagesById.get(id);
		if (!name) {
			throw new Error(`Cargo default workspace member is unknown: ${id}`);
		}
		return name;
	});

	const disabled = names.filter((name) => disabledBrowserPackages.has(name));
	if (disabled.length > 0) {
		throw new Error(
			`disabled browser packages must not be Cargo default members: ${disabled.join(", ")}`,
		);
	}
	if (names.length === 0) {
		throw new Error("Cargo default workspace member list is empty");
	}

	return [...new Set(names)].sort((left, right) => left.localeCompare(right));
}

export function rustfmtCheckArgs(metadata) {
	return [
		"fmt",
		"--check",
		...defaultRustfmtPackages(metadata).flatMap((name) => ["--package", name]),
	];
}

function parseArgs(argv) {
	let root = defaultRoot;
	for (let index = 0; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg === "--root") {
			const value = argv[++index];
			if (!value) throw new Error("--root requires a path");
			root = value;
			continue;
		}
		if (arg.startsWith("--root=")) {
			root = arg.slice("--root=".length);
			continue;
		}
		throw new Error(`unknown argument: ${arg}`);
	}
	return { root: resolve(root) };
}

export function main(argv = process.argv.slice(2)) {
	const { root } = parseArgs(argv);
	if (!existsSync(resolve(root, "Cargo.toml"))) {
		throw new Error(`Cargo.toml not found under ${root}`);
	}
	const metadata = readCargoMetadata(root);
	execFileSync("cargo", rustfmtCheckArgs(metadata), {
		cwd: root,
		stdio: "inherit",
	});
	console.log(
		`Rust formatting ok (${defaultRustfmtPackages(metadata).length} non-browser packages)`,
	);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
	main();
}
