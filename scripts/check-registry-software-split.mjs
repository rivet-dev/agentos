import {
	existsSync,
	readdirSync,
	readFileSync,
	statSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const agentOsPackagePattern = /^@rivet-dev\/agent-os(?:-|$)/;
const dependencySections = [
	"dependencies",
	"devDependencies",
	"peerDependencies",
	"optionalDependencies",
];

function formatPath(root, path) {
	return relative(root, path).replaceAll("\\", "/") || ".";
}

function readJson(path) {
	return JSON.parse(readFileSync(path, "utf8"));
}

function collectSoftwareDirs(root) {
	const softwareRoot = join(root, "registry", "software");
	if (!existsSync(softwareRoot)) {
		return [];
	}

	return readdirSync(softwareRoot, { withFileTypes: true })
		.filter((entry) => entry.isDirectory() && !entry.name.startsWith("_"))
		.map((entry) => join(softwareRoot, entry.name))
		.filter((packageDir) => {
			const manifestPath = join(packageDir, "package.json");
			return existsSync(manifestPath) && statSync(manifestPath).isFile();
		})
		.sort((left, right) => left.localeCompare(right));
}

function checkDependencies(packageName, manifest, errors) {
	for (const section of dependencySections) {
		const dependencies = manifest[section];
		if (!dependencies || typeof dependencies !== "object") {
			continue;
		}
		for (const dependencyName of Object.keys(dependencies)) {
			if (agentOsPackagePattern.test(dependencyName)) {
				errors.push(
					`${packageName} must not depend on Agent OS package ${dependencyName} in registry software ${section}`,
				);
			}
		}
	}
}

export function checkRegistrySoftwareSplit(options = {}) {
	const root = resolve(options.root ?? defaultRoot);
	const errors = [];

	for (const packageDir of collectSoftwareDirs(root)) {
		const dirName = packageDir.split(/[\\/]/).at(-1);
		const manifestPath = join(packageDir, "package.json");
		const metadataPath = join(packageDir, "secure-exec-package.json");
		const staleMetadataPath = join(packageDir, "agentos-package.json");
		const staleArtifactMetadataPath = join(
			packageDir,
			"agentos-package.meta.json",
		);

		const manifest = readJson(manifestPath);
		const expectedName = `@agentos-software/${dirName}`;
		if (manifest.name !== expectedName) {
			errors.push(
				`${formatPath(root, manifestPath)} must be named ${expectedName}, found ${manifest.name}`,
			);
		}

		if (existsSync(staleMetadataPath)) {
			errors.push(
				`${formatPath(root, staleMetadataPath)} must be renamed to secure-exec-package.json`,
			);
		}
		if (existsSync(staleArtifactMetadataPath)) {
			errors.push(
				`${formatPath(root, staleArtifactMetadataPath)} must be renamed to secure-exec-package.meta.json`,
			);
		}
		if (!existsSync(metadataPath)) {
			errors.push(`${formatPath(root, metadataPath)} is required`);
		} else {
			const metadata = readJson(metadataPath);
			if (metadata.name !== manifest.name) {
				errors.push(
					`${formatPath(root, metadataPath)} name must match package.json (${manifest.name}), found ${metadata.name}`,
				);
			}
		}

		checkDependencies(manifest.name ?? expectedName, manifest, errors);
	}

	return errors;
}

function parseArgs(argv) {
	let root = defaultRoot;
	for (let i = 0; i < argv.length; i++) {
		const arg = argv[i];
		if (arg === "--root") {
			const value = argv[++i];
			if (!value) {
				throw new Error("--root requires a path");
			}
			root = value;
			continue;
		}
		if (arg.startsWith("--root=")) {
			root = arg.slice("--root=".length);
			continue;
		}
		throw new Error(`unknown argument: ${arg}`);
	}
	return { root };
}

export function main(argv = process.argv.slice(2)) {
	const { root } = parseArgs(argv);
	const resolvedRoot = resolve(root);
	if (!existsSync(resolvedRoot) || !statSync(resolvedRoot).isDirectory()) {
		throw new Error(`root is not a directory: ${resolvedRoot}`);
	}

	const errors = checkRegistrySoftwareSplit({ root: resolvedRoot });
	if (errors.length > 0) {
		for (const error of errors) {
			console.error(error);
		}
		process.exitCode = 1;
		return;
	}

	console.log("registry software split check ok");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	main();
}
