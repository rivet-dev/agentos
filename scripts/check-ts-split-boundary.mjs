import {
	existsSync,
	readdirSync,
	readFileSync,
	statSync,
} from "node:fs";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const defaultAgentOsRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const defaultSecureExecRoot = resolve(defaultAgentOsRoot, "..", "secure-exec");

const requiredCoreExports = [
	"./protocol",
	"./native-client",
	"./sidecar-client",
	"./protocol-frames",
];
const requiredAgentOsExports = [
	"AgentOs",
	"AgentOsSidecar",
	"CronManager",
	"binding",
	"bindingGroup",
	"defineSoftware",
];
const requiredAgentOsMethods = [
	"create",
	"createSidecar",
	"exec",
	"spawn",
	"readFile",
	"writeFile",
	"writeFiles",
	"readFiles",
	"mkdir",
	"readdir",
	"stat",
	"exists",
	"fetch",
	"openShell",
	"connectTerminal",
	"createSession",
	"destroySession",
	"prompt",
	"cancelSession",
	"closeSession",
	"respondPermission",
	"setSessionMode",
	"getSessionModes",
	"rawSessionSend",
	"rawSend",
	"onSessionEvent",
	"scheduleCron",
	"listCronJobs",
	"cancelCronJob",
	"onCronEvent",
	"dispose",
];

function readJson(path) {
	return JSON.parse(readFileSync(path, "utf8"));
}

function readText(path) {
	return readFileSync(path, "utf8");
}

function packageJsonHasExports(manifest, exports) {
	if (!manifest.exports || typeof manifest.exports !== "object") {
		return false;
	}
	return exports.every((exportName) =>
		Object.prototype.hasOwnProperty.call(manifest.exports, exportName),
	);
}

function dependencySpec(manifest, name) {
	for (const section of [
		"dependencies",
		"devDependencies",
		"peerDependencies",
		"optionalDependencies",
	]) {
		const dependencies = manifest[section];
		if (dependencies && typeof dependencies === "object" && name in dependencies) {
			return dependencies[name];
		}
	}
	return undefined;
}

function sourceFiles(root) {
	const files = [];
	function walk(dir) {
		if (!existsSync(dir)) {
			return;
		}
		for (const entry of readdirSync(dir, { withFileTypes: true })) {
			const path = join(dir, entry.name);
			if (entry.isDirectory()) {
				if (!["node_modules", "dist", "target", ".git", ".jj"].includes(entry.name)) {
					walk(path);
				}
				continue;
			}
			if (entry.isFile() && [".ts", ".tsx", ".mts", ".cts"].includes(extname(entry.name))) {
				files.push(path);
			}
		}
	}
	walk(root);
	return files;
}

function hasAnySourceMatch(root, patterns) {
	for (const file of sourceFiles(root)) {
		const source = readText(file);
		for (const { label, pattern } of patterns) {
			pattern.lastIndex = 0;
			if (pattern.test(source)) {
				return `${relative(root, file).replaceAll("\\", "/")} matches ${label}`;
			}
		}
	}
	return null;
}

function sourceHasMethod(source, name) {
	const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	return new RegExp(`\\b(?:static\\s+)?(?:async\\s+)?${escaped}\\s*\\(`).test(source);
}

function check(name, ok, details) {
	return { name, ok, details };
}

export function auditTsSplitBoundary(options = {}) {
	const agentOsRoot = resolve(options.agentOsRoot ?? defaultAgentOsRoot);
	const secureExecRoot = resolve(options.secureExecRoot ?? defaultSecureExecRoot);
	const agentOsCoreRoot = join(agentOsRoot, "packages/core");
	const secureExecCoreRoot = join(secureExecRoot, "packages/core");
	const checks = [];

	const secureExecCoreManifestPath = join(secureExecCoreRoot, "package.json");
	const agentOsCoreManifestPath = join(agentOsCoreRoot, "package.json");
	const secureExecCoreManifest = existsSync(secureExecCoreManifestPath)
		? readJson(secureExecCoreManifestPath)
		: {};
	const agentOsCoreManifest = existsSync(agentOsCoreManifestPath)
		? readJson(agentOsCoreManifestPath)
		: {};
	const secureExecCoreIndex = existsSync(join(secureExecCoreRoot, "src/index.ts"))
		? readText(join(secureExecCoreRoot, "src/index.ts"))
		: "";
	const secureExecGeneratedProtocol = existsSync(
		join(secureExecCoreRoot, "src/generated-protocol.ts"),
	)
		? readText(join(secureExecCoreRoot, "src/generated-protocol.ts"))
		: "";
	const agentOsCoreIndex = existsSync(join(agentOsCoreRoot, "src/index.ts"))
		? readText(join(agentOsCoreRoot, "src/index.ts"))
		: "";
	const agentOsSource = existsSync(join(agentOsCoreRoot, "src/agent-os.ts"))
		? readText(join(agentOsCoreRoot, "src/agent-os.ts"))
		: "";
	const nativeProcessClient = existsSync(
		join(agentOsCoreRoot, "src/sidecar/native-process-client.ts"),
	)
		? readText(join(agentOsCoreRoot, "src/sidecar/native-process-client.ts"))
		: "";

	checks.push(
		check(
			"@secure-exec/core is the generic TS core package",
			secureExecCoreManifest.name === "@secure-exec/core" &&
				packageJsonHasExports(secureExecCoreManifest, requiredCoreExports),
			relative(secureExecRoot, secureExecCoreManifestPath),
		),
		check(
			"@secure-exec/core exposes generated sidecar protocol types",
			secureExecGeneratedProtocol.includes("export type ProtocolFrame") &&
				secureExecGeneratedProtocol.includes("export function readProtocolFrame") &&
				secureExecGeneratedProtocol.includes("export function writeProtocolFrame") &&
				secureExecCoreIndex.includes('export * as protocol from "./generated-protocol.js"') &&
				secureExecCoreIndex.includes('export * from "./generated-protocol.js"'),
			relative(secureExecRoot, join(secureExecCoreRoot, "src/generated-protocol.ts")),
		),
	);

	const forbiddenSecureExecMatch = hasAnySourceMatch(secureExecCoreRoot, [
		{ label: "@rivet-dev/agentos import", pattern: /@rivet-dev\/agent-os/g },
		{ label: "AgentOs facade", pattern: /\bclass\s+AgentOs\b|\bexport\s+\{\s*AgentOs\b/g },
		{ label: "Agent OS host-bindings sugar", pattern: /\bbinding\b|\bbindingGroup\b|\bzodToJsonSchema\b/g },
		{ label: "Agent OS cron sugar", pattern: /\bCronManager\b|\bTimerScheduleDriver\b/g },
		{ label: "Agent OS defineSoftware sugar", pattern: /\bdefineSoftware\b/g },
	]);
	checks.push(
		check(
			"@secure-exec/core has no Agent OS facade or TS-only sugar",
			forbiddenSecureExecMatch === null,
			forbiddenSecureExecMatch ?? "none",
		),
	);

	const secureExecCoreDeps = [
		"@anthropic-ai/claude-agent-sdk",
		"@mariozechner/pi-coding-agent",
		"croner",
		"zod",
		"@copilotkit/llmock",
	];
	checks.push(
		check(
			"@secure-exec/core dependencies stay generic",
			secureExecCoreDeps.every((name) => dependencySpec(secureExecCoreManifest, name) === undefined),
			relative(secureExecRoot, secureExecCoreManifestPath),
		),
	);

	const secureExecDependency = dependencySpec(agentOsCoreManifest, "@secure-exec/core");
	checks.push(
		check(
			"@rivet-dev/agentos-core depends on published @secure-exec/core",
			agentOsCoreManifest.name === "@rivet-dev/agentos-core" &&
				typeof secureExecDependency === "string" &&
				secureExecDependency === "catalog:",
			secureExecDependency ?? "missing",
		),
		check(
			"@rivet-dev/agentos-core exports the AgentOs facade and sugar",
			requiredAgentOsExports.every((name) => agentOsCoreIndex.includes(name)),
			relative(agentOsRoot, join(agentOsCoreRoot, "src/index.ts")),
		),
		check(
			"Agent OS native process client delegates to @secure-exec/core sidecar client",
			nativeProcessClient.includes('from "@secure-exec/core/sidecar-client"'),
			relative(agentOsRoot, join(agentOsCoreRoot, "src/sidecar/native-process-client.ts")),
		),
	);

	for (const file of [
		"src/host-bindings.ts",
		"src/host-bindings-zod.ts",
		"src/packages.ts",
		"src/cron/index.ts",
		"src/cron/cron-manager.ts",
		"src/sidecar/agentos-protocol.ts",
	]) {
		const path = join(agentOsCoreRoot, file);
		checks.push(
			check(
				`@rivet-dev/agentos-core keeps ${file}`,
				existsSync(path) && statSync(path).isFile(),
				relative(agentOsRoot, path),
			),
		);
	}

	const missingMethods = requiredAgentOsMethods.filter(
		(method) => !sourceHasMethod(agentOsSource, method),
	);
	checks.push(
		check(
			"AgentOs facade keeps required public method surface",
			missingMethods.length === 0,
			missingMethods.length === 0 ? "all required methods present" : missingMethods.join(", "),
		),
		check(
			"Agent OS facade uses generated ACP and secure-exec transport boundaries",
			agentOsSource.includes('from "./sidecar/agentos-protocol.js"') &&
				agentOsSource.includes('from "./sidecar/rpc-client.js"') &&
				agentOsSource.includes('from "@secure-exec/core/descriptors"') &&
				agentOsSource.includes("extensionRequest("),
			relative(agentOsRoot, join(agentOsCoreRoot, "src/agent-os.ts")),
		),
	);

	return {
		agentOsRoot,
		secureExecRoot,
		ready: checks.every((item) => item.ok),
		checks,
	};
}

function parseArgs(argv) {
	const options = {
		agentOsRoot: defaultAgentOsRoot,
		secureExecRoot: defaultSecureExecRoot,
		expectReady: false,
	};
	for (let i = 0; i < argv.length; i++) {
		const arg = argv[i];
		if (arg === "--agentos-root") {
			options.agentOsRoot = argv[++i];
			continue;
		}
		if (arg.startsWith("--agentos-root=")) {
			options.agentOsRoot = arg.slice("--agentos-root=".length);
			continue;
		}
		if (arg === "--secure-exec-root") {
			options.secureExecRoot = argv[++i];
			continue;
		}
		if (arg.startsWith("--secure-exec-root=")) {
			options.secureExecRoot = arg.slice("--secure-exec-root=".length);
			continue;
		}
		if (arg === "--expect-ready") {
			options.expectReady = true;
			continue;
		}
		throw new Error(`unknown argument: ${arg}`);
	}
	return options;
}

export function main(argv = process.argv.slice(2)) {
	const options = parseArgs(argv);
	const result = auditTsSplitBoundary(options);
	for (const item of result.checks) {
		const prefix = item.ok ? "ok" : "not ok";
		console.log(`${prefix} - ${item.name}: ${item.details}`);
	}
	if (options.expectReady && !result.ready) {
		console.error("TS split boundary is not ready");
		return 1;
	}
	return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	process.exitCode = main();
}
