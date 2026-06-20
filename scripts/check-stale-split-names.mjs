import {
	existsSync,
	readdirSync,
	readFileSync,
	statSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const scannedFileNames = new Set([
	".env",
	".env.example",
	".npmrc",
	".yarnrc.yml",
	"Cargo.lock",
	"Cargo.toml",
	"package.json",
	"pnpm-lock.yaml",
	"pnpm-workspace.yaml",
	"turbo.json",
]);

const scannedExtensions = new Set([
	".cjs",
	".cts",
	".js",
	".json",
	".md",
	".mjs",
	".mts",
	".rs",
	".sh",
	".ts",
	".tsx",
	".toml",
	".yaml",
	".yml",
]);

const ignoredDirectories = new Set([
	".git",
	".jj",
	".turbo",
	"coverage",
	"dist",
	"node_modules",
	"target",
]);

const ignoredPathPrefixes = [
	"crates/execution/assets/pyodide/",
	"crates/execution/assets/generated/",
	"crates/v8-runtime/assets/generated/",
	"scripts/ralph/archive/",
	"scripts/ralph/codex-streams/",
	"website/.astro/",
];

const ignoredFiles = new Set([
	"scripts/check-agent-os-client-protocol-compat.test.mjs",
	"scripts/check-secure-exec-package-boundary.mjs",
	"scripts/check-secure-exec-package-boundary.test.mjs",
	"scripts/check-stale-split-names.mjs",
	"scripts/check-stale-split-names.test.mjs",
]);

const stalePatterns = [
	{
		name: "legacy stdin env var",
		pattern: /\bAGENT_OS_KEEP_STDIN_OPEN\b/g,
		replacement: "SECURE_EXEC_KEEP_STDIN_OPEN",
	},
	{
		name: "legacy sidecar binary env var",
		pattern: /\bAGENT_OS_SIDECAR_BINARY\b/g,
		replacement: "AGENT_OS_SIDECAR_BIN",
	},
	{
		name: "legacy secure-exec repo path",
		pattern: /(?:~|\/home\/[^/\s"'`]+|\.\.)\/se1\b/g,
		replacement: "../secure-exec or ~/secure-exec",
	},
	{
		name: "legacy Agent OS command projection path",
		pattern: /\/__agentos\/(?:commands|node-runtime)\b/g,
		replacement: "/__secure_exec/{commands,node-runtime}",
	},
	{
		name: "compat protocol version constant",
		pattern: /\bsecure_exec_client::protocol::PROTOCOL_VERSION\b/g,
		replacement: "secure_exec_client::wire::PROTOCOL_VERSION",
	},
	{
		name: "compat protocol name constant",
		pattern: /\bsecure_exec_client::protocol::PROTOCOL_NAME\b/g,
		replacement: "secure_exec_client::wire::PROTOCOL_NAME",
	},
	{
		name: "stale agent-os-client wire-surface documentation",
		pattern:
			/all\s+wire\s+types\s+are\s+reused\s+from\s+`secure_exec_client::protocol`/g,
		replacement:
			"document secure_exec_client::wire as the generated schema surface",
	},
	{
		name: "legacy session resume API",
		pattern: /\bresumeSession\b/g,
		replacement: "live sessions created through createSession",
	},
	{
		name: "legacy session event history API",
		pattern: /\bgetSessionEvents\b/g,
		replacement: "live onSessionEvent subscriptions",
	},
	{
		name: "legacy sequenced session event type",
		pattern: /\bSequencedEvent\b/g,
		replacement: "live session events",
	},
	{
		name: "legacy session event options type",
		pattern: /\bGetEventsOptions\b/g,
		replacement: "live onSessionEvent subscriptions",
	},
	{
		name: "legacy acknowledged session event replay docs",
		pattern:
			/\b(?:ack-based|acknowledged event replay|acknowledged high-water mark|bounded sequenced-event buffer)\b/g,
		replacement: "live-only session events",
	},
	{
		name: "legacy host callback registration wire name",
		pattern: /\bregister_toolkit\b/g,
		replacement: "registerHostCallbacks or RegisterHostCallbacks",
	},
	{
		name: "legacy registered tool input schema wording",
		pattern: /\bregistered tool `input_schema`/g,
		replacement: "registered host callback `input_schema`",
	},
	{
		name: "legacy core ACP implementation path",
		pattern: /crates\/sidecar\/src\/acp(?:\/(?:client|session)\.rs|\/)?/g,
		replacement: "crates/agent-os-sidecar/src/acp_extension.rs",
		pathPattern: /\.md$/,
	},
	{
		name: "legacy core ACP create-session guidance",
		pattern: /crates\/sidecar\/src\/service\.rs[^.\n]*\bCreateSession\b/g,
		replacement:
			"crates/agent-os-sidecar/src/acp_extension.rs create-session handling",
		pathPattern: /\.md$/,
	},
	{
		name: "legacy core ACP orchestration guidance",
		pattern: /ACP orchestration embedded in `service\.rs`/g,
		replacement: "ACP orchestration embedded in `acp_extension.rs`",
		pathPattern: /\.md$/,
	},
	{
		name: "legacy core ACP callback payload",
		pattern:
			/\bSidecar(?:RequestPayload::AcpRequest|ResponsePayload::AcpRequestResult)\b/g,
		replacement: "ACP Ext callbacks",
		pathPattern: /\.md$/,
	},
	{
		name: "legacy core ACP session state",
		pattern: /\b(?:AcpSessionState|close_agent_session)\b/g,
		replacement: "Agent OS ACP extension session records",
		pathPattern: /\.md$/,
	},
	{
		name: "legacy ACP client error surface",
		pattern: /\bAcpClientError\b/g,
		replacement: "SidecarError propagation from the Agent OS ACP extension",
		pathPattern: /\.md$/,
	},
	{
		name: "legacy manual BARE discriminant guidance",
		pattern:
			/\b(?:must use explicit schema discriminants|manual Rust tag mappings|preserve the existing human-readable JSON encoding for the migration window)\b/g,
		replacement: "generated positional tag layout",
		pathPattern: /\.md$/,
	},
];

const requiredFileContents = [
	{
		path: "crates/sidecar/src/wire.rs",
		expected: 'pub const PROTOCOL_NAME: &str = "secure-exec-sidecar";',
		description: "Rust secure-exec protocol schema name",
	},
	{
		path: "crates/sidecar/protocol/README.md",
		expected: "`ProtocolSchema.name` is `secure-exec-sidecar`",
		description: "secure-exec protocol schema documentation",
	},
	{
		path: "crates/sidecar/protocol/README.md",
		expected: "`ProtocolSchema.version` is `7`",
		description: "secure-exec protocol schema version documentation",
	},
];

function formatPath(root, path) {
	return relative(root, path) || ".";
}

function shouldIgnorePath(root, path) {
	const rel = formatPath(root, path).replaceAll("\\", "/");
	return (
		ignoredFiles.has(rel) ||
		ignoredPathPrefixes.some((prefix) => rel.startsWith(prefix))
	);
}

function shouldScanFile(path) {
	const name = path.split(/[\\/]/).at(-1);
	if (scannedFileNames.has(name)) {
		return true;
	}
	for (const extension of scannedExtensions) {
		if (path.endsWith(extension)) {
			return true;
		}
	}
	return false;
}

function collectFiles(root, dir = root) {
	if (!existsSync(dir) || shouldIgnorePath(root, dir)) {
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
		if (entry.isFile() && shouldScanFile(path) && !shouldIgnorePath(root, path)) {
			files.push(path);
		}
	}
	return files;
}

function lineAndColumn(source, index) {
	const prefix = source.slice(0, index);
	const lines = prefix.split("\n");
	return {
		line: lines.length,
		column: lines.at(-1).length + 1,
	};
}

export function checkStaleSplitNames(options = {}) {
	const root = resolve(options.root ?? defaultRoot);
	const files = (options.files ?? collectFiles(root)).toSorted();
	const errors = [];

	for (const filePath of files) {
		const source = readFileSync(filePath, "utf8");
		for (const stale of stalePatterns) {
			const relativePath = formatPath(root, filePath).replaceAll("\\", "/");
			if (stale.pathPattern && !stale.pathPattern.test(relativePath)) {
				continue;
			}
			stale.pattern.lastIndex = 0;
			for (const match of source.matchAll(stale.pattern)) {
				const location = lineAndColumn(source, match.index ?? 0);
				errors.push(
					`${formatPath(root, filePath)}:${location.line}:${location.column} uses ${stale.name} ${match[0]}; use ${stale.replacement}`,
				);
			}
		}
	}

	for (const required of requiredFileContents) {
		const filePath = join(root, required.path);
		if (!existsSync(filePath)) {
			continue;
		}
		const source = readFileSync(filePath, "utf8");
		if (!source.includes(required.expected)) {
			errors.push(
				`${required.path} has stale ${required.description}; expected ${JSON.stringify(required.expected)}`,
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

	const errors = checkStaleSplitNames({ root: resolvedRoot });
	if (errors.length > 0) {
		for (const error of errors) {
			console.error(error);
		}
		process.exitCode = 1;
		return;
	}

	console.log("stale split-name check ok");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	main();
}
