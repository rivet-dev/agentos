import {
	existsSync,
	readdirSync,
	readFileSync,
	statSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const forbiddenSpecifiers = new Set([
	"@rivet-dev/agentos-core/test/runtime",
	"@rivet-dev/agentos-core/internal/runtime-compat",
	"@secure-exec/core/test-runtime",
]);
const allowedRegistryHelper = "registry/tests/helpers.ts";
const scannedExtensions = new Set([
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
]);
const importSpecifierPatterns = [
	/\bimport\s+(?:type\s+)?(?:[^"'()]*?\s+from\s+)?["']([^"']+)["']/g,
	/\bexport\s+(?:type\s+)?[^"'()]*?\s+from\s+["']([^"']+)["']/g,
	/\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
	/\brequire\s*\(\s*["']([^"']+)["']\s*\)/g,
];

function formatPath(root, path) {
	return relative(root, path).replaceAll("\\", "/") || ".";
}

function shouldScanFile(path) {
	for (const extension of scannedExtensions) {
		if (path.endsWith(extension)) {
			return true;
		}
	}
	return false;
}

function collectFiles(root, dir) {
	if (!existsSync(dir)) {
		return [];
	}

	const files = [];
	for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
		a.name.localeCompare(b.name),
	)) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) {
			if (!ignoredDirectories.has(entry.name)) {
				files.push(...collectFiles(root, path));
			}
			continue;
		}
		if (entry.isFile() && shouldScanFile(path)) {
			files.push(path);
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

export function checkRegistryTestRuntimeBoundary(options = {}) {
	const root = resolve(options.root ?? defaultRoot);
	const registryTestsDir = join(root, "registry", "tests");
	const files = (options.files ?? collectFiles(root, registryTestsDir)).toSorted();
	const errors = [];

	for (const filePath of files) {
		const rel = formatPath(root, filePath);
		if (rel === allowedRegistryHelper) {
			continue;
		}
		const source = readFileSync(filePath, "utf8");
		const forbiddenImport = collectImportSpecifiers(source).find((specifier) =>
			forbiddenSpecifiers.has(specifier),
		);
		if (forbiddenImport) {
			errors.push(
				`${rel} must import registry test runtime helpers from ../helpers.js instead of ${forbiddenImport}`,
			);
		}
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

	const errors = checkRegistryTestRuntimeBoundary({ root: resolvedRoot });
	if (errors.length > 0) {
		for (const error of errors) {
			console.error(error);
		}
		process.exitCode = 1;
		return;
	}

	console.log("registry test runtime boundary ok");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	main();
}
