import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const guidanceRoots = [
	"website/src/content/docs/docs",
	"website/public/docs/docs",
];

const forbiddenClaims = [
	{
		id: "permission-product-deny-default",
		pattern: /\b(?:deny[- ]by[- ]default|default[- ]deny)\b/i,
	},
	{
		id: "permission-everything-denied",
		pattern: /\beverything is denied until (?:explicitly )?opted in\b/i,
	},
	{
		id: "permission-nothing-allowed",
		pattern: /\bnothing is allowed until you opt in\b/i,
	},
	{
		id: "permission-nothing-bound",
		pattern: /\bnothing is bound by default\b.*\baccess is denied\b/i,
	},
	{
		id: "permission-omission-denies",
		pattern:
			/\b(?:omitted (?:permissions|scopes?)\s+(?:(?:are|become)\s+)?(?:deny|denies|denied)|omitted (?:permissions|scopes?)\s+(?:default|resolve)\w*\s+to\s+deny|(?:permissions|scopes?)\s+(?:(?:are|become)\s+)?denied\s+when omitted)\b/i,
	},
	{
		id: "permission-process-default-deny",
		pattern: /\bprocess execution is denied by default\b/i,
	},
	{
		id: "permission-network-default-deny",
		pattern:
			/\b(?:by default[^.]*guest cannot reach the network|network (?:access )?is denied[^.]*opt in)\b/i,
	},
	{
		id: "permission-secure-default-network-deny",
		pattern: /\bsecure default\b[^.]*\bden(?:y|ies|ied)\b[^.]*\bnetwork\b/i,
	},
	{
		id: "permission-scope-table-default-deny",
		paths: new Set([
			"website/src/content/docs/docs/permissions.mdx",
			"website/public/docs/docs/permissions.md",
		]),
		pattern:
			/^\|\s*`?(?:fs|network|childProcess|process|env|binding)`?\s*\|.*\|\s*`?deny`?\*?\s*\|\s*$/i,
	},
];

const requiredClaims = new Map([
	["README.md", ["omitted permissions allow all"]],
	[
		"website/src/content/docs/docs/permissions.mdx",
		[
			"omitting permissions selects the sidecar owned allow all product default",
			"omitted top level scopes inherit allow",
			"omitted default means deny",
		],
	],
	[
		"website/public/docs/docs/permissions.md",
		[
			"omitting permissions selects the sidecar owned allow all product default",
			"omitted top level scopes inherit allow",
			"omitted default means deny",
		],
	],
	[
		"website/src/content/docs/docs/security-model.mdx",
		["sidecar resolves omitted permissions to allow all"],
	],
	[
		"website/public/docs/docs/security-model.md",
		["sidecar resolves omitted permissions to allow all"],
	],
	[
		"website/src/content/docs/docs/networking.mdx",
		["network operations are allowed when permissions or its network scope is omitted"],
	],
	[
		"website/public/docs/docs/networking.md",
		["network operations are allowed when permissions or its network scope is omitted"],
	],
	[
		"website/src/content/docs/docs/python-runtime.mdx",
		["network is allowed when permissions or its network scope is omitted"],
	],
	[
		"website/public/docs/docs/python-runtime.md",
		["network is allowed when permissions or its network scope is omitted"],
	],
	[
		"website/src/content/docs/docs/architecture.mdx",
		["sidecar resolves omitted top level scopes to allow"],
	],
	[
		"website/public/docs/docs/architecture.md",
		["sidecar resolves omitted top level scopes to allow"],
	],
	[
		"website/src/content/docs/docs/architecture/filesystem.mdx",
		["sidecar installs allow when the top level fs scope is omitted"],
	],
	[
		"website/public/docs/docs/architecture/filesystem.md",
		["sidecar installs allow when the top level fs scope is omitted"],
	],
	[
		"website/src/content/docs/docs/architecture/processes.mdx",
		["sidecar installs allow when childprocess is omitted"],
	],
	[
		"website/public/docs/docs/architecture/processes.md",
		["sidecar installs allow when childprocess is omitted"],
	],
	[
		"website/src/content/docs/docs/versus-sandbox.mdx",
		["allow all when omitted"],
	],
	[
		"website/public/docs/docs/versus-sandbox.md",
		["allow all when omitted"],
	],
]);

function parseArgs(argv) {
	const options = { root: defaultRoot };
	for (let index = 0; index < argv.length; index++) {
		const argument = argv[index];
		if (argument === "--root") {
			const root = argv[++index];
			if (root === undefined) throw new Error("--root requires a path");
			options.root = root;
			continue;
		}
		if (argument.startsWith("--root=")) {
			const root = argument.slice("--root=".length);
			if (root.length === 0) throw new Error("--root requires a path");
			options.root = root;
			continue;
		}
		throw new Error(`unknown argument: ${argument}`);
	}
	return { root: resolve(options.root) };
}

function toRelative(root, path) {
	return relative(root, path).split(sep).join("/");
}

function walkGuidance(root, directory, files) {
	for (const entry of readdirSync(directory, { withFileTypes: true })) {
		const path = join(directory, entry.name);
		if (entry.isDirectory()) {
			walkGuidance(root, path, files);
			continue;
		}
		if (!entry.isFile() || !/\.(?:md|mdx)$/.test(entry.name)) continue;
		files.push({ path, relativePath: toRelative(root, path) });
	}
}

function stripFencedCode(lines) {
	let fence;
	return lines.map((line) => {
		const marker = line.match(/^\s*(```|~~~)/)?.[1];
		if (fence === undefined && marker !== undefined) {
			fence = marker;
			return "";
		}
		if (fence !== undefined) {
			if (marker === fence) fence = undefined;
			return "";
		}
		return line;
	});
}

function normalizeClaim(text) {
	return text.toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();
}

function recommendsRestrictivePolicy(line) {
	return (
		/\bexplicit\b.*\b(?:policy|rule(?:[- ]set)?)\b/i.test(line) ||
		/\b(?:use|pass|configure|create)\b.*\bdeny[- ]by[- ]default\b.*\b(?:policy|rule(?:[- ]set)?)\b/i.test(
			line,
		)
	);
}

export function auditThinClientDocs(options = {}) {
	const root = resolve(options.root ?? defaultRoot);
	const failures = [];
	const files = [];
	const readFiles = new Map();

	const readGuidance = (relativePath) => {
		if (readFiles.has(relativePath)) return readFiles.get(relativePath);
		const path = join(root, relativePath);
		if (!existsSync(path)) return undefined;
		const text = readFileSync(path, "utf8");
		readFiles.set(relativePath, text);
		return text;
	};

	if (existsSync(join(root, "README.md"))) {
		files.push({ path: join(root, "README.md"), relativePath: "README.md" });
	}
	for (const relativeRoot of guidanceRoots) {
		const directory = join(root, relativeRoot);
		if (existsSync(directory)) walkGuidance(root, directory, files);
	}
	files.sort((left, right) => left.relativePath.localeCompare(right.relativePath));

	for (const { path, relativePath } of files) {
		const text = readFileSync(path, "utf8");
		readFiles.set(relativePath, text);
		const lines = text.split(/\r?\n/);
		const prose = stripFencedCode(lines);
		for (let index = 0; index < prose.length; index++) {
			for (const rule of forbiddenClaims) {
				if (rule.paths !== undefined && !rule.paths.has(relativePath)) continue;
				if (
					rule.id === "permission-product-deny-default" &&
					recommendsRestrictivePolicy(prose[index])
				) {
					continue;
				}
				if (!rule.pattern.test(prose[index])) continue;
				failures.push({
					path: relativePath,
					line: index + 1,
					ruleId: rule.id,
					text: lines[index].trim(),
				});
			}
		}
	}

	for (const [relativePath, fragments] of requiredClaims) {
		const text = readGuidance(relativePath);
		if (text === undefined) {
			failures.push({
				path: relativePath,
				line: 0,
				ruleId: "required-guidance-file",
				text: "required guidance file is missing",
			});
			continue;
		}
		const normalized = normalizeClaim(stripFencedCode(text.split(/\r?\n/)).join("\n"));
		for (const fragment of fragments) {
			if (normalized.includes(normalizeClaim(fragment))) continue;
			failures.push({
				path: relativePath,
				line: 0,
				ruleId: "required-allow-all-claim",
				text: `missing claim: ${fragment}`,
			});
		}
	}

	failures.sort(
		(left, right) =>
			left.path.localeCompare(right.path) ||
			left.line - right.line ||
			left.ruleId.localeCompare(right.ruleId),
	);
	return {
		root,
		ok: failures.length === 0,
		filesChecked: files.length,
		failures,
	};
}

export function main(argv = process.argv.slice(2)) {
	const result = auditThinClientDocs(parseArgs(argv));
	if (result.ok) {
		process.stdout.write(
			`verify-thin-client-docs: OK (${result.filesChecked} guidance files checked)\n`,
		);
		return 0;
	}
	for (const failure of result.failures) {
		const location = failure.line === 0 ? failure.path : `${failure.path}:${failure.line}`;
		process.stderr.write(
			`verify-thin-client-docs: ${location} [${failure.ruleId}] ${failure.text}\n`,
		);
	}
	return 1;
}

if (
	process.argv[1] !== undefined &&
	import.meta.url === pathToFileURL(process.argv[1]).href
) {
	try {
		process.exitCode = main();
	} catch (error) {
		process.stderr.write(`verify-thin-client-docs: ${error.message}\n`);
		process.exitCode = 1;
	}
}
