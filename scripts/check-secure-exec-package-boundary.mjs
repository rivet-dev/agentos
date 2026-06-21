import {
	existsSync,
	readdirSync,
	readFileSync,
	statSync,
} from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const agentOsPackagePattern = /^@rivet-dev\/agent-os(?:-|$)/;
const compatibilityPackages = new Set([
	"secure-exec",
	"@secure-exec/typescript",
]);
const dependencySections = [
	"dependencies",
	"devDependencies",
	"peerDependencies",
	"optionalDependencies",
];
const scannedSourceExtensions = new Set([
	".js",
	".jsx",
	".mjs",
	".cjs",
	".ts",
	".tsx",
	".mts",
	".cts",
]);
const ignoredDirectories = new Set([
	"dist",
	"node_modules",
	"coverage",
	".turbo",
	".vitest",
	"_secure-exec-sibling",
]);

const importSpecifierPatterns = [
	/\bimport\s+(?:type\s+)?(?:[^"'()]*?\s+from\s+)?["']([^"']+)["']/g,
	/\bexport\s+(?:type\s+)?[^"'()]*?\s+from\s+["']([^"']+)["']/g,
	/\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
	/\brequire\s*\(\s*["']([^"']+)["']\s*\)/g,
];
const agentOsFacadeSymbolPatterns = [
	{ label: "AgentOs", pattern: /\bAgentOs\b/ },
	{ label: "HostTool", pattern: /\bHostTool\b/ },
	{ label: "ToolKit", pattern: /\bToolKit\b/ },
	{ label: "registerToolkit", pattern: /\bregisterToolkit\b/ },
	{ label: "register_toolkit", pattern: /\bregister_toolkit\b/ },
	{ label: "toolkit_registered", pattern: /\btoolkit_registered\b/ },
	{
		label: "SidecarRegisteredToolDefinition",
		pattern: /\bSidecarRegisteredToolDefinition\b/,
	},
	{
		label: "SidecarRegisteredToolExample",
		pattern: /\bSidecarRegisteredToolExample\b/,
	},
];
const agentOsDocumentationPatterns = [
	{ label: "Agent OS", pattern: /\bAgent OS\b/g },
	{ label: "AgentOs", pattern: /\bAgentOs\b/g },
	{ label: "@rivet-dev/agentos", pattern: /@rivet-dev\/agent-os(?:-|$)/g },
];

function readJson(path) {
	return JSON.parse(readFileSync(path, "utf8"));
}

function isSecureExecPackage(packageName) {
	return (
		typeof packageName === "string" &&
		(packageName.startsWith("@secure-exec/") ||
			compatibilityPackages.has(packageName))
	);
}

function isAuditedPackage(packageName) {
	return isSecureExecPackage(packageName) && !compatibilityPackages.has(packageName);
}

function collectPackageDirs(root, dir = root) {
	if (!existsSync(dir)) {
		return [];
	}

	const packageDirs = [];
	const manifestPath = join(dir, "package.json");
	if (existsSync(manifestPath) && statSync(manifestPath).isFile()) {
		packageDirs.push(dir);
	}

	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (!entry.isDirectory() || ignoredDirectories.has(entry.name)) {
			continue;
		}
		packageDirs.push(...collectPackageDirs(root, join(dir, entry.name)));
	}
	return packageDirs;
}

function collectSourceFiles(dir) {
	if (!existsSync(dir)) {
		return [];
	}

	const files = [];
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) {
			if (!ignoredDirectories.has(entry.name)) {
				files.push(...collectSourceFiles(path));
			}
			continue;
		}

		if (!entry.isFile()) {
			continue;
		}

		for (const extension of scannedSourceExtensions) {
			if (entry.name.endsWith(extension)) {
				files.push(path);
				break;
			}
		}
	}
	return files;
}

function collectImportSpecifiers(source) {
	const specifiers = [];
	for (const pattern of importSpecifierPatterns) {
		pattern.lastIndex = 0;
		for (const match of source.matchAll(pattern)) {
			specifiers.push(match[1]);
		}
	}
	return specifiers;
}

function formatPath(root, path) {
	return relative(root, path) || ".";
}

function isPathWithinDir(path, dir) {
	return path === dir || path.startsWith(`${dir}/`);
}

function checkManifestDependencies(packageName, manifest, errors) {
	for (const section of dependencySections) {
		const dependencies = manifest[section];
		if (!dependencies || typeof dependencies !== "object") {
			continue;
		}
		for (const dependencyName of Object.keys(dependencies)) {
			if (agentOsPackagePattern.test(dependencyName)) {
				errors.push(
					`${packageName} must not depend on Agent OS package ${dependencyName} (${section})`,
				);
			}
		}
	}
}

function checkManifestDocumentation(packageName, manifest, manifestPath, root, errors) {
	if (typeof manifest.description !== "string") {
		return;
	}
	for (const { label, pattern } of agentOsDocumentationPatterns) {
		pattern.lastIndex = 0;
		if (pattern.test(manifest.description)) {
			errors.push(
				`${packageName} package description must not mention Agent OS surface ${label} (${formatPath(root, manifestPath)})`,
			);
		}
	}
}

function collectExportTargets(exportsField) {
	const targets = new Set();

	function visit(value) {
		if (typeof value === "string") {
			targets.add(value);
			return;
		}
		if (!value || typeof value !== "object" || Array.isArray(value)) {
			return;
		}
		for (const nested of Object.values(value)) {
			visit(nested);
		}
	}

	visit(exportsField);
	return targets;
}

function checkExportedSourceSurface(root, packageDir, packageName, manifest, errors) {
	if (manifest.private === true && packageName.startsWith("@secure-exec/example-")) {
		return;
	}

	const srcDir = join(packageDir, "src");
	if (!existsSync(srcDir)) {
		return;
	}

	if (!manifest.exports || typeof manifest.exports !== "object") {
		errors.push(`${packageName} must declare package.json exports for src/`);
		return;
	}

	const exportedTargets = collectExportTargets(manifest.exports);
	for (const filePath of collectSourceFiles(srcDir)) {
		const relativeSource = relative(srcDir, filePath);
		if (relativeSource.includes("/") || relativeSource.includes("\\")) {
			continue;
		}

		const moduleName = basename(relativeSource).replace(
			/\.(?:c|m)?(?:t|j)sx?$/,
			"",
		);
		const expectedJsTarget = `./dist/${moduleName}.js`;
		if (!exportedTargets.has(expectedJsTarget)) {
			errors.push(
				`${packageName} must export ${formatPath(root, filePath)} through package.json exports`,
			);
		}
	}
}

function checkSourceImports(root, packageDirs, packageDir, packageName, errors) {
	for (const filePath of collectSourceFiles(packageDir)) {
		const source = readFileSync(filePath, "utf8");
		for (const specifier of collectImportSpecifiers(source)) {
			if (agentOsPackagePattern.test(specifier)) {
				errors.push(
					`${packageName} must not import Agent OS package ${specifier} (${formatPath(root, filePath)})`,
				);
				continue;
			}

			if (!specifier.startsWith(".")) {
				continue;
			}

			const targetPath = resolve(dirname(filePath), specifier);
			if (isPathWithinDir(targetPath, packageDir)) {
				continue;
			}
			const foreignPackageDir = packageDirs.find(
				(candidate) =>
					candidate !== packageDir &&
					!isPathWithinDir(packageDir, candidate) &&
					isPathWithinDir(targetPath, candidate),
			);
			if (foreignPackageDir) {
				errors.push(
					`${packageName} must not import source from another package via ${specifier} (${formatPath(root, filePath)})`,
				);
			}
		}
	}
}

function checkAgentOsFacadeSymbols(root, packageDir, packageName, errors) {
	for (const filePath of collectSourceFiles(packageDir)) {
		const source = readFileSync(filePath, "utf8");
		for (const { label, pattern } of agentOsFacadeSymbolPatterns) {
			pattern.lastIndex = 0;
			if (pattern.test(source)) {
				errors.push(
					`${packageName} must not expose Agent OS facade/toolkit symbol ${label} (${formatPath(root, filePath)})`,
				);
			}
		}
	}
}

function checkPackageReadme(root, packageDir, packageName, errors) {
	const readmePath = join(packageDir, "README.md");
	if (!existsSync(readmePath)) {
		return;
	}
	const source = readFileSync(readmePath, "utf8");
	for (const { label, pattern } of agentOsDocumentationPatterns) {
		pattern.lastIndex = 0;
		if (pattern.test(source)) {
			errors.push(
				`${packageName} README must not mention Agent OS surface ${label} (${formatPath(root, readmePath)})`,
			);
		}
	}
}

function checkSecureExecBaseFilesystem(packageDir, packageName, errors) {
	if (packageName !== "@secure-exec/core") {
		return;
	}

	const fixturePath = join(packageDir, "fixtures/base-filesystem.json");
	if (!existsSync(fixturePath)) {
		return;
	}

	const fixture = readJson(fixturePath);
	const hostname = fixture?.environment?.env?.HOSTNAME;
	if (hostname !== "secure-exec") {
		errors.push(
			`${packageName} base filesystem HOSTNAME must be secure-exec, got ${JSON.stringify(hostname)}`,
		);
	}

	const hostnameEntry = fixture?.filesystem?.entries?.find(
		(entry) => entry?.path === "/etc/hostname",
	);
	if (hostnameEntry?.content !== "secure-exec\n") {
		errors.push(
			`${packageName} base filesystem /etc/hostname must contain secure-exec`,
		);
	}

	const transforms = fixture?.source?.transforms;
	if (
		Array.isArray(transforms) &&
		transforms.some((item) => typeof item === "string" && item.includes("AgentOs"))
	) {
		errors.push(`${packageName} base filesystem metadata must not mention AgentOs`);
	}
}

function checkSecureExecCoreRootSurface(root, packageDir, packageName, errors) {
	if (packageName !== "@secure-exec/core") {
		return;
	}

	const indexPath = join(packageDir, "src/index.ts");
	if (!existsSync(indexPath)) {
		return;
	}

	const source = readFileSync(indexPath, "utf8");
	for (const specifier of collectImportSpecifiers(source)) {
		if (specifier === "./sidecar-client.js") {
			errors.push(
				`${packageName} root export must not re-export ./sidecar-client; keep it on the explicit subpath (${formatPath(root, indexPath)})`,
			);
		}
	}
}

export function checkSecureExecPackageBoundary(options = {}) {
	const root = resolve(options.root ?? defaultRoot);
	const errors = [];
	const packageDirs = collectPackageDirs(root);

	for (const packageDir of packageDirs) {
		const manifestPath = join(packageDir, "package.json");
		if (!existsSync(manifestPath)) {
			continue;
		}

		const manifest = readJson(manifestPath);
		const packageName = manifest.name;
		if (!isAuditedPackage(packageName)) {
			continue;
		}

		checkManifestDependencies(packageName, manifest, errors);
		checkManifestDocumentation(packageName, manifest, manifestPath, root, errors);
		checkExportedSourceSurface(root, packageDir, packageName, manifest, errors);
		checkSourceImports(root, packageDirs, packageDir, packageName, errors);
		checkAgentOsFacadeSymbols(root, packageDir, packageName, errors);
		checkPackageReadme(root, packageDir, packageName, errors);
		checkSecureExecBaseFilesystem(packageDir, packageName, errors);
		checkSecureExecCoreRootSurface(root, packageDir, packageName, errors);
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
	const options = parseArgs(argv);
	const errors = checkSecureExecPackageBoundary(options);
	if (errors.length > 0) {
		for (const error of errors) {
			console.error(error);
		}
		process.exitCode = 1;
		return;
	}

	console.log("secure-exec package boundary ok");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	main();
}
