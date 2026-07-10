#!/usr/bin/env node

/**
 * Pi SDK ACP Adapter
 *
 * ACP-compliant adapter that embeds the Pi coding agent SDK directly
 * instead of spawning a subprocess. This avoids loading ~100MB of TUI
 * code that the CLI pulls in even in headless mode.
 *
 * Speaks ACP JSON-RPC over stdin/stdout using @agentclientprotocol/sdk.
 * Internally builds a real Pi AgentSession without loading the CLI's
 * resource loader path, which pulls jiti into the VM runtime.
 */

import {
	type Agent,
	AgentSideConnection,
	type RequestError,
	ndJsonStream,
} from "@agentclientprotocol/sdk";
import type {
	AuthenticateRequest,
	AuthenticateResponse,
	CancelNotification,
	InitializeRequest,
	InitializeResponse,
	NewSessionRequest,
	NewSessionResponse,
	PromptRequest,
	PromptResponse,
	SetSessionModeRequest,
	SetSessionModeResponse,
	SessionNotification,
} from "@agentclientprotocol/sdk";
import type {
	AgentSessionEvent,
} from "@mariozechner/pi-coding-agent";
import {
	existsSync,
	readFileSync,
	readdirSync,
	writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { isAbsolute, join, resolve as resolvePath } from "node:path";
import { PassThrough } from "node:stream";

// ── Phase tracing (opt-in via PI_TRACE_FILE) ────────────────────────
// Emits Chrome-trace ("X") events for newSession sub-phases so the otherwise
// opaque ACP `session/new` span can be broken down and compared against the
// bare-node equivalent. The file is written in the guest VFS and read by the
// host (e.g. the benchmark) via vm.readFile.
function startPhaseTrace() {
	const spans: { name: string; ts: number; dur: number }[] = [];
	const t0 = Date.now();
	return {
		async span<T>(name: string, fn: () => Promise<T> | T): Promise<T> {
			const start = Date.now();
			try {
				return await fn();
			} finally {
				spans.push({ name, ts: (start - t0) * 1000, dur: (Date.now() - start) * 1000 });
			}
		},
		flush(extra: Record<string, unknown> = {}) {
			const file = process.env.PI_TRACE_FILE;
			if (!file) return;
			const events = [
				{ name: "newSession", cat: "pi", ph: "X", pid: 1, tid: 1, ts: 0, dur: (Date.now() - t0) * 1000, args: extra },
				...spans.map((s) => ({ name: s.name, cat: "pi", ph: "X", pid: 1, tid: 1, ts: s.ts, dur: s.dur })),
			];
			try {
				writeFileSync(file, JSON.stringify(events));
			} catch {
				/* tracing is best-effort */
			}
		},
	};
}

const PI_SDK_PACKAGE = "@mariozechner/pi-coding-agent";
const MODULE_ACCESS_NODE_MODULES = "/root/node_modules";
const require = createRequire(import.meta.url);

const realStdin = process.stdin;
const bufferedStdin = new PassThrough();
(bufferedStdin as PassThrough & { isTTY?: boolean; fd?: number }).isTTY =
	realStdin.isTTY;
(bufferedStdin as PassThrough & { isTTY?: boolean; fd?: number }).fd =
	realStdin.fd;
if (typeof realStdin.setRawMode === "function") {
	(
		bufferedStdin as PassThrough & {
			setRawMode?: (mode: boolean) => void;
		}
	).setRawMode = realStdin.setRawMode.bind(realStdin);
}
Object.defineProperty(process, "stdin", {
	configurable: true,
	enumerable: true,
	value: bufferedStdin,
});

type SessionManagerLike = {
	inMemory(cwd?: string): unknown;
	continueRecent(cwd: string, sessionDir: string): unknown;
};

/** Select persistent Pi sessions only when the adapter is explicitly configured. */
export function resolveSessionManager(
	SessionManager: SessionManagerLike,
	cwd: string,
	env: Record<string, string | undefined> = process.env,
): unknown {
	const sessionDir = env.PI_SESSION_DIR?.trim();
	return sessionDir
		? SessionManager.continueRecent(cwd, sessionDir)
		: SessionManager.inMemory(cwd);
}

type ModelLike = {
	id: string;
	provider: string;
	baseUrl?: string;
	reasoning?: boolean;
};

type MinimalResourceLoaderLike = {
	reload(): Promise<void>;
	getExtensions(): {
		extensions: unknown[];
		errors: unknown[];
		runtime: {
			flagValues: Map<string, unknown>;
			pendingProviderRegistrations: Array<{
				name: string;
				config: unknown;
			}>;
		};
	};
	getSkills(): { skills: unknown[]; diagnostics: unknown[] };
	getPrompts(): { prompts: unknown[]; diagnostics: unknown[] };
	getThemes(): { themes: unknown[]; diagnostics: unknown[] };
	getAgentsFiles(): { agentsFiles: unknown[] };
	getSystemPrompt(): string;
	getAppendSystemPrompt(): string[];
	getPathMetadata(): Map<string, unknown>;
	extendResources(_paths: string[]): void;
};

type PiAgentCoreLike = new (config: {
	initialState: {
		systemPrompt: string;
		model: ModelLike | undefined;
		thinkingLevel: string;
		tools: unknown[];
	};
	convertToLlm: (messages: unknown[]) => unknown[];
	onPayload: (payload: unknown, model: unknown) => Promise<unknown>;
	sessionId: string;
	transformContext: (messages: unknown[]) => Promise<unknown[]>;
	steeringMode: unknown;
	followUpMode: unknown;
	transport: unknown;
	thinkingBudgets: unknown;
	maxRetryDelayMs: number;
	getApiKey: (provider?: string) => Promise<string>;
}) => {
	state: {
		model?: ModelLike;
		thinkingLevel: string;
	};
	subscribe(listener: (event: unknown) => void): () => void;
	prompt(text: string): Promise<void>;
	abort(): void;
	setThinkingLevel(level: string): void;
	setTools(tools: PiToolLike[]): void;
	setSystemPrompt(prompt: string): void;
	replaceMessages(messages: unknown[]): void;
};

type SettingsManagerInstanceLike = {
	getDefaultProvider(): string | undefined;
	getDefaultModel(): string | undefined;
	getDefaultThinkingLevel(): string | undefined;
	getBlockImages(): boolean;
	getSteeringMode(): unknown;
	getFollowUpMode(): unknown;
	getTransport(): unknown;
	getThinkingBudgets(): unknown;
	getRetrySettings(): { maxDelayMs: number };
	getShellCommandPrefix(): string | undefined;
	getImageAutoResize(): boolean;
};

type ModelRegistryInstanceLike = {
	find(provider: string, modelId: string): ModelLike | undefined;
	getAvailable(): Promise<ModelLike[]>;
	getApiKey(model: ModelLike): Promise<string | undefined>;
	getApiKeyForProvider(provider: string): Promise<string | undefined>;
	isUsingOAuth(model: ModelLike): boolean;
};

type SessionManagerInstanceLike = {
	buildSessionContext(): {
		messages: unknown[];
		model?: { provider: string; modelId: string };
		thinkingLevel?: string;
	};
	getBranch(): Array<{ type: string }>;
	appendModelChange(provider: string, modelId: string): void;
	appendThinkingLevelChange(thinkingLevel: string): void;
	getSessionId(): string;
};

type PiToolLike = {
	name: string;
	description?: string;
	parameters?: unknown;
	execute(
		toolCallId: string,
		args: unknown,
		signal: AbortSignal,
		onUpdate?: (partialResult: unknown) => void,
	): Promise<{
		content: unknown;
		details?: unknown;
	}>;
};

type ExtensionFactoryLike = (api: unknown) => unknown;

type PiSessionLike = {
	readonly sessionId: string;
	readonly thinkingLevel: string;
	readonly messages: unknown[];
	subscribe(
		listener: (event: AgentSessionEvent) => void,
	): () => void;
	getAvailableThinkingLevels(): string[];
	prompt(text: string): Promise<void>;
	abort(): Promise<void>;
	setThinkingLevel(level: string): void;
};

type PiSdkRuntime = {
	Agent: PiAgentCoreLike;
	AuthStorage: {
		create(authPath?: string): unknown;
	};
	DefaultResourceLoader: new (options: {
		cwd?: string;
		agentDir?: string;
		settingsManager?: SettingsManagerInstanceLike;
		appendSystemPrompt?: string;
		extensionFactories?: ExtensionFactoryLike[];
		noExtensions?: boolean;
	}) => MinimalResourceLoaderLike;
	DEFAULT_THINKING_LEVEL: string;
	ModelRegistry: new (authStorage: unknown, modelsPath?: string) => {
		find(provider: string, modelId: string): ModelLike | undefined;
		getAvailable(): Promise<ModelLike[]>;
		getApiKey(model: ModelLike): Promise<string | undefined>;
		getApiKeyForProvider(provider: string): Promise<string | undefined>;
		isUsingOAuth(model: ModelLike): boolean;
	};
	SettingsManager: {
		create(cwd?: string, agentDir?: string): SettingsManagerInstanceLike;
	};
	SessionManager: SessionManagerLike;
	convertToLlm(messages: unknown[]): unknown[];
	getAgentDir(): string;
	getDocsPath(): string;
	createAgentSession(options?: {
		cwd?: string;
		agentDir?: string;
		sessionManager?: unknown;
		resourceLoader?: MinimalResourceLoaderLike;
		settingsManager?: SettingsManagerInstanceLike;
		tools?: PiToolLike[];
		customTools?: PiToolLike[];
	}): Promise<{ session: PiSessionLike; modelFallbackMessage?: string }>;
	createCodingTools(
		cwd: string,
		options?: {
			read?: { autoResizeImages?: boolean };
			bash?: {
				commandPrefix?: string;
			};
		},
	): PiToolLike[];
	createAllTools(
		cwd: string,
		options?: {
			read?: { autoResizeImages?: boolean };
			bash?: {
				commandPrefix?: string;
			};
		},
	): Record<string, PiToolLike>;
};

let piSdkRuntimePromise: Promise<PiSdkRuntime> | undefined;

class MinimalPiSession implements PiSessionLike {
	private readonly listeners = new Set<
		(event: AgentSessionEvent) => void
	>();

	constructor(
		private readonly agent: InstanceType<PiAgentCoreLike>,
		private readonly sessionManager: SessionManagerInstanceLike,
		private readonly settingsManager: SettingsManagerInstanceLike,
		private readonly resourceLoader: MinimalResourceLoaderLike,
		private readonly runtime: Pick<PiSdkRuntime, "createAllTools">,
		private readonly cwd: string,
		private readonly appendPrompt?: string,
	) {
		this.agent.subscribe((event) => {
			this.emit(event as AgentSessionEvent);
		});
		this.rebuildRuntime();
	}

	get sessionId(): string {
		return this.sessionManager.getSessionId();
	}

	get thinkingLevel(): string {
		return this.agent.state.thinkingLevel;
	}

	get messages(): unknown[] {
		return (this.agent as { state: { messages?: unknown[] } }).state.messages ?? [];
	}

	subscribe(listener: (event: AgentSessionEvent) => void): () => void {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}

	getAvailableThinkingLevels(): string[] {
		return this.agent.state.model?.reasoning
			? ["off", "minimal", "low", "medium", "high"]
			: ["off"];
	}

	async prompt(text: string): Promise<void> {
		await this.agent.prompt(text);
	}

	async abort(): Promise<void> {
		this.agent.abort();
	}

	setThinkingLevel(level: string): void {
		const nextLevel = this.agent.state.model?.reasoning ? level : "off";
		this.agent.setThinkingLevel(nextLevel);
		this.sessionManager.appendThinkingLevelChange(nextLevel);
	}

	private emit(event: AgentSessionEvent): void {
		for (const listener of this.listeners) {
			listener(event);
		}
	}

	private rebuildRuntime(): void {
		const baseTools = this.runtime.createAllTools(this.cwd, {
			read: {
				autoResizeImages: this.settingsManager.getImageAutoResize(),
			},
			bash: {
				commandPrefix: this.settingsManager.getShellCommandPrefix(),
			},
		});
		const activeToolNames = ["read", "bash", "edit", "write"].filter(
			(name) => name in baseTools,
		);
		this.agent.setTools(
			activeToolNames.map((name) => baseTools[name]).filter(Boolean),
		);
		this.agent.setSystemPrompt(
			buildAdapterSystemPrompt(this.cwd, this.appendPrompt),
		);
	}
}

function buildAdapterSystemPrompt(
	cwd: string,
	appendPrompt?: string,
): string {
	const date = new Date().toISOString().slice(0, 10);
	const extra = appendPrompt ? `\n\n${appendPrompt}` : "";
	return (
		"You are an expert coding assistant operating inside Pi's ACP adapter.\n" +
		"Use the available tools when they help complete the user's request.\n" +
		"Be concise, prefer direct file and shell operations, and describe file paths clearly." +
		`${extra}\nCurrent date: ${date}\nCurrent working directory: ${cwd}`
	);
}

const DISCOVERED_EXTENSION_INDEX_CANDIDATES = [
	"index.js",
	"index.mjs",
	"index.cjs",
] as const;

function isDiscoveredExtensionEntry(name: string): boolean {
	return (
		name.endsWith(".js") || name.endsWith(".mjs") || name.endsWith(".cjs")
	);
}

function discoverAutoExtensionPaths(cwd: string, agentDir: string): string[] {
	const extensionRoots = [join(agentDir, "extensions"), join(cwd, ".pi", "extensions")];
	const discovered = new Set<string>();

	for (const root of extensionRoots) {
		if (!existsSync(root)) {
			continue;
		}
		for (const entry of readdirSync(root, { withFileTypes: true })) {
			const entryPath = join(root, entry.name);
			if (entry.isFile() && isDiscoveredExtensionEntry(entry.name)) {
				discovered.add(entryPath);
				continue;
			}
			if (!entry.isDirectory()) {
				continue;
			}
			for (const candidate of DISCOVERED_EXTENSION_INDEX_CANDIDATES) {
				const candidatePath = join(entryPath, candidate);
				if (existsSync(candidatePath)) {
					discovered.add(candidatePath);
					break;
				}
			}
		}
	}

	return [...discovered].sort();
}

function readCommonJsExtensionFactory(
	extensionPath: string,
): ExtensionFactoryLike | undefined {
	const required = require(extensionPath);
	if (typeof required === "function") {
		return required as ExtensionFactoryLike;
	}
	if (typeof required?.default === "function") {
		return required.default as ExtensionFactoryLike;
	}
	return undefined;
}

// Temporary workaround: the V8 module loader currently fails dynamic
// import() of ESM `.js` extension files, so this evaluates a transformed
// copy of bare `export default` extensions. It cannot handle `import`
// statements or named exports. Delete this once the loader supports ESM
// `.js` dynamic import.
function readInlineDefaultExportFactory(
	extensionPath: string,
): ExtensionFactoryLike | undefined {
	const source = readFileSync(extensionPath, "utf8");
	if (!/\bexport\s+default\b/.test(source)) {
		return undefined;
	}

	const module = { exports: {} as { default?: unknown } };
	const transformed = source.replace(
		/\bexport\s+default\b/,
		"module.exports.default =",
	);
	new Function("module", "exports", "require", transformed)(
		module,
		module.exports,
		require,
	);

	return typeof module.exports.default === "function"
		? (module.exports.default as ExtensionFactoryLike)
		: undefined;
}

async function loadExtensionFactoryFromPath(
	extensionPath: string,
): Promise<ExtensionFactoryLike | undefined> {
	if (extensionPath.endsWith(".cjs")) {
		return readCommonJsExtensionFactory(extensionPath);
	}

	if (extensionPath.endsWith(".mjs")) {
		const module = await import(extensionPath);
		return typeof module.default === "function"
			? (module.default as ExtensionFactoryLike)
			: undefined;
	}

	try {
		return readCommonJsExtensionFactory(extensionPath);
	} catch (error) {
		let inlineFactory: ExtensionFactoryLike | undefined;
		try {
			inlineFactory = readInlineDefaultExportFactory(extensionPath);
		} catch (inlineError) {
			const inlineMessage =
				inlineError instanceof Error ? inlineError.message : String(inlineError);
			if (error instanceof Error) {
				error.message = `${error.message} (inline default-export fallback also failed: ${inlineMessage})`;
			}
			throw error;
		}
		if (inlineFactory) {
			return inlineFactory;
		}
		throw error;
	}
}

async function loadDiscoveredExtensionFactories(
	cwd: string,
	agentDir: string,
): Promise<{
	extensionFactories: ExtensionFactoryLike[];
	errors: Array<{ path: string; error: string }>;
}> {
	const extensionFactories: ExtensionFactoryLike[] = [];
	const errors: Array<{ path: string; error: string }> = [];

	for (const extensionPath of discoverAutoExtensionPaths(cwd, agentDir)) {
		try {
			const factory = await loadExtensionFactoryFromPath(extensionPath);
			if (!factory) {
				errors.push({
					path: extensionPath,
					error: "Extension does not export a valid factory function",
				});
				continue;
			}
			extensionFactories.push(factory);
		} catch (error) {
			errors.push({
				path: extensionPath,
				error: error instanceof Error ? error.message : String(error),
			});
		}
	}

	return { extensionFactories, errors };
}

class MinimalResourceLoader implements MinimalResourceLoaderLike {
	private readonly runtime = {
		flagValues: new Map<string, unknown>(),
		pendingProviderRegistrations: [] as Array<{
			name: string;
			config: unknown;
		}>,
	};

	constructor(private readonly options: { appendSystemPrompt?: string }) {}

	async reload(): Promise<void> {}

	getExtensions() {
		return {
			extensions: [],
			errors: [],
			runtime: this.runtime,
		};
	}

	getSkills() {
		return { skills: [], diagnostics: [] };
	}

	getPrompts() {
		return { prompts: [], diagnostics: [] };
	}

	getThemes() {
		return { themes: [], diagnostics: [] };
	}

	getAgentsFiles() {
		return { agentsFiles: [] };
	}

	getSystemPrompt(): string {
		return "";
	}

	getAppendSystemPrompt(): string[] {
		return this.options.appendSystemPrompt ? [this.options.appendSystemPrompt] : [];
	}

	getPathMetadata(): Map<string, unknown> {
		return new Map();
	}

	extendResources(_paths: string[]): void {}
}

function findInstalledPackageRoot(packageName: string): string | null {
	const searchPaths = require.resolve.paths(packageName) ?? [];
	for (const basePath of searchPaths) {
		const candidateRoot = join(basePath, packageName);
		if (existsSync(join(candidateRoot, "package.json"))) {
			return candidateRoot;
		}
	}
	return null;
}

function findProjectedPackageRoot(packageName: string): string {
	const installedRoot = findInstalledPackageRoot(packageName);
	if (installedRoot) {
		return installedRoot;
	}

	const directRoot = `${MODULE_ACCESS_NODE_MODULES}/${packageName}`;
	const pnpmRoot = `${MODULE_ACCESS_NODE_MODULES}/.pnpm`;
	const pnpmPrefix = `${packageName.replace("/", "+")}@`;

	if (existsSync(pnpmRoot)) {
		for (const entry of readdirSync(pnpmRoot)) {
			if (!entry.startsWith(pnpmPrefix)) continue;
			const candidateRoot = join(pnpmRoot, entry, "node_modules", packageName);
			if (existsSync(join(candidateRoot, "package.json"))) {
				return candidateRoot;
			}
		}
	}

	return directRoot;
}

// When the agent SDK has been evaluated into the V8 startup snapshot (built once
// per sidecar), its runtime API is already present on this global — published by
// the snapshot build-entry (snapshot-entry.ts). Reading it skips the per-session
// dynamic-import + evaluate of the whole SDK graph entirely.
const PI_SDK_RUNTIME_GLOBAL = "__PI_SDK_RUNTIME__";

function readSnapshotRuntime(): PiSdkRuntime | undefined {
	const candidate = (globalThis as Record<string, unknown>)[
		PI_SDK_RUNTIME_GLOBAL
	];
	if (
		candidate &&
		typeof candidate === "object" &&
		typeof (candidate as PiSdkRuntime).createAgentSession === "function"
	) {
		return candidate as PiSdkRuntime;
	}
	return undefined;
}

async function loadPiSdkRuntime(): Promise<PiSdkRuntime> {
	if (!piSdkRuntimePromise) {
		// Snapshot fast path: the SDK runtime is already on the global (evaluated
		// into the snapshot at sidecar startup). No I/O, no module resolution.
		const snapshotRuntime = readSnapshotRuntime();
		if (snapshotRuntime) {
			piSdkRuntimePromise = Promise.resolve(snapshotRuntime);
			return piSdkRuntimePromise;
		}
		// Fallback: load the SDK from the guest VFS via dynamic import (the path
		// used when no snapshot is present, e.g. a cold/unsupported runtime).
		piSdkRuntimePromise = (async () => {
			const packageRoot = findProjectedPackageRoot(PI_SDK_PACKAGE);
			const agentCoreRoot = findProjectedPackageRoot("@mariozechner/pi-agent-core");
			const [
				agentCoreModule,
				authStorageModule,
				configModule,
				defaultsModule,
				messagesModule,
				modelRegistryModule,
				resourceLoaderModule,
				sdkModule,
				sessionManagerModule,
				settingsManagerModule,
				toolsModule,
			] =
				await Promise.all([
					import(`${agentCoreRoot}/dist/index.js`),
					import(`${packageRoot}/dist/core/auth-storage.js`),
					import(`${packageRoot}/dist/config.js`),
					import(`${packageRoot}/dist/core/defaults.js`),
					import(`${packageRoot}/dist/core/messages.js`),
					import(`${packageRoot}/dist/core/model-registry.js`),
					import(`${packageRoot}/dist/core/resource-loader.js`),
					import(`${packageRoot}/dist/core/sdk.js`),
					import(`${packageRoot}/dist/core/session-manager.js`),
					import(`${packageRoot}/dist/core/settings-manager.js`),
					import(`${packageRoot}/dist/core/tools/index.js`),
				]);

			return {
				Agent: agentCoreModule.Agent as PiAgentCoreLike,
				AuthStorage: authStorageModule.AuthStorage as PiSdkRuntime["AuthStorage"],
				DefaultResourceLoader:
					resourceLoaderModule.DefaultResourceLoader as PiSdkRuntime["DefaultResourceLoader"],
				DEFAULT_THINKING_LEVEL:
					defaultsModule.DEFAULT_THINKING_LEVEL as string,
				ModelRegistry:
					modelRegistryModule.ModelRegistry as PiSdkRuntime["ModelRegistry"],
				SettingsManager:
					settingsManagerModule.SettingsManager as PiSdkRuntime["SettingsManager"],
				SessionManager: sessionManagerModule.SessionManager as SessionManagerLike,
				convertToLlm:
					messagesModule.convertToLlm as PiSdkRuntime["convertToLlm"],
				getAgentDir: configModule.getAgentDir as PiSdkRuntime["getAgentDir"],
				getDocsPath: configModule.getDocsPath as PiSdkRuntime["getDocsPath"],
				createAgentSession:
					sdkModule.createAgentSession as PiSdkRuntime["createAgentSession"],
				createCodingTools:
					sdkModule.createCodingTools as PiSdkRuntime["createCodingTools"],
				createAllTools:
					toolsModule.createAllTools as PiSdkRuntime["createAllTools"],
			};
		})();
	}

	return piSdkRuntimePromise;
}

async function createAgentSession(options: {
	cwd: string;
	sessionManager: unknown;
	resourceLoader: MinimalResourceLoaderLike;
	tools?: PiToolLike[];
}): Promise<{ session: PiSessionLike; modelFallbackMessage?: string }> {
	const { createAgentSession: createPiAgentSession, SettingsManager } =
		await loadPiSdkRuntime();

	const cwd = options.cwd;
	const homeDir = process.env.HOME || "/home/agentos";
	const agentDir = join(homeDir, ".pi", "agent");
	const settingsManager = SettingsManager.create(cwd, agentDir);
	const result = await createPiAgentSession({
		cwd,
		agentDir,
		sessionManager: options.sessionManager,
		resourceLoader: options.resourceLoader,
		settingsManager,
		tools: options.tools,
		customTools: options.tools,
	});
	applyAnthropicBaseUrlOverride(result.session);
	return result;
}

function applyAnthropicBaseUrlOverride(session: PiSessionLike): void {
	const baseUrl = process.env.ANTHROPIC_BASE_URL;
	if (!baseUrl) return;
	const agent = (session as { agent?: { state?: { model?: ModelLike } } }).agent;
	const model = agent?.state?.model;
	if (model?.provider !== "anthropic") return;
	if (!agent?.state) return;
	agent.state.model = { ...model, baseUrl };
}

// ── CLI argument parsing ────────────────────────────────────────────

let appendSystemPrompt: string | undefined;
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
	if (argv[i] === "--append-system-prompt" && i + 1 < argv.length) {
		appendSystemPrompt = argv[i + 1];
		i++;
	}
}

// ── Agent implementation ────────────────────────────────────────────

// Exported for unit tests (drive the translation handlers with a mock
// connection + a fake session). Not part of the package's public API.
export class PiSdkAgent implements Agent {
	private conn: AgentSideConnection;
	private session: PiSessionLike | null = null;
	private sessionId = "";
	private cwd = "/workspace";
	private cancelRequested = false;
	private currentToolCalls = new Map<string, string>();
	private emittedAssistantText = false;
	private bufferingUpdates = false;
	private pendingUpdates: SessionNotification["update"][] = [];
	private streamedTextContent = new Set<string>();
	private editSnapshots = new Map<
		string,
		{ path: string; oldText: string }
	>();
	private lastEmit: Promise<void> = Promise.resolve();
	private unsubscribe: (() => void) | null = null;
	private disposed = false;

	constructor(conn: AgentSideConnection) {
		this.conn = conn;
		// The ACP connection ending is the only unconditional teardown signal for
		// this adapter (there is no SDK-invoked close hook). Drop the live Pi event
		// subscription on connection close so a closed connection doesn't leave a
		// listener — and the session it closes over — alive. Defer to the next tick:
		// AgentSideConnection invokes this agent factory mid-construction, so
		// `conn.closed` is not yet readable synchronously here (matches the claude
		// adapter's pattern).
		setTimeout(() => {
			void this.conn.closed.then(() => this.dispose());
		}, 0);
	}

	private dispose(): void {
		this.disposed = true;
		if (this.unsubscribe) {
			this.unsubscribe();
			this.unsubscribe = null;
		}
		this.session = null;
	}

	async initialize(
		_params: InitializeRequest,
	): Promise<InitializeResponse> {
		return {
			protocolVersion: 1,
			agentInfo: {
				name: "pi-sdk-acp",
				title: "Pi SDK ACP adapter",
				version: "0.1.0",
			},
			agentCapabilities: {
				promptCapabilities: {
					image: true,
					audio: false,
					embeddedContext: false,
				},
			},
		};
	}

	async newSession(
		params: NewSessionRequest,
	): Promise<NewSessionResponse> {
		const __trace = startPhaseTrace();
		this.cwd = params.cwd;
		process.chdir(params.cwd);
		const agentDir = join(process.env.HOME || "/home/agentos", ".pi", "agent");
		const {
			DefaultResourceLoader,
			SessionManager,
			SettingsManager,
			createCodingTools,
		} = await __trace.span("loadPiSdkRuntime", () => loadPiSdkRuntime());
		const { extensionFactories, errors: extensionLoadErrors } =
			await __trace.span("loadExtensions", () =>
				loadDiscoveredExtensionFactories(params.cwd, agentDir),
			);
		// Step 3: when no workspace extensions were discovered (the common case),
		// skip DefaultResourceLoader entirely — it eagerly loads skills/themes/
		// prompts/agentsFiles (~250ms) the headless ACP adapter doesn't use. The
		// MinimalResourceLoader's reload() is a no-op. Only the full loader (with its
		// extension runtime) is constructed when extensions are actually present.
		const resourceLoader: MinimalResourceLoaderLike =
			extensionFactories.length > 0
				? new DefaultResourceLoader({
						cwd: params.cwd,
						agentDir,
						noExtensions: true,
						extensionFactories,
						...(appendSystemPrompt ? { appendSystemPrompt } : {}),
					})
				: new MinimalResourceLoader({
						...(appendSystemPrompt ? { appendSystemPrompt } : {}),
					});
		await __trace.span("resourceLoader.reload", () => resourceLoader.reload());
		for (const { path, error } of extensionLoadErrors) {
			console.warn(`[pi-sdk-acp] Failed to load extension ${path}: ${error}`);
		}
		const settingsManager = SettingsManager.create(
			params.cwd,
			agentDir,
		);

		const { session } = await __trace.span("createAgentSession", () =>
			createAgentSession({
				cwd: params.cwd,
				sessionManager: resolveSessionManager(SessionManager, params.cwd),
				resourceLoader,
				tools: this.wrapTools(
					createCodingTools(params.cwd, {
						read: {
							autoResizeImages: settingsManager.getImageAutoResize(),
						},
						bash: {
							commandPrefix: settingsManager.getShellCommandPrefix(),
						},
					}),
				),
			}),
		);
		__trace.flush();

		// Replacing the session: drop the previous session's subscription so its
		// listener (and the session it closes over) doesn't leak or fire against
		// stale state.
		if (this.unsubscribe) {
			this.unsubscribe();
			this.unsubscribe = null;
		}
		this.session = session;
		this.sessionId = session.sessionId;

		// Subscribe to Pi SDK events and translate to ACP notifications. Keep the
		// disposer so the subscription is torn down on session replace / connection
		// close (`subscribe` returns an unsubscribe function).
		this.unsubscribe = session.subscribe((event) => this.handlePiEvent(event));

		// Build thinking modes
		const thinkingLevels = session.getAvailableThinkingLevels();
		const modes = {
			currentModeId: session.thinkingLevel,
			availableModes: thinkingLevels.map((id) => ({
				id,
				name: `Thinking: ${id}`,
			})),
		};

		return {
			sessionId: this.sessionId,
			modes,
		};
	}

	async prompt(params: PromptRequest): Promise<PromptResponse> {
		const session = this.session;
		if (!session) {
			throw new Error("No session created");
		}

		this.cancelRequested = false;
		this.currentToolCalls.clear();
		this.emittedAssistantText = false;
		this.pendingUpdates = [];
		this.streamedTextContent.clear();
		// Pre-edit snapshots are per-tool-call, captured at tool_execution_start and
		// consumed at tool_execution_end. If a tool never reaches `end` (cancel /
		// crash / abort) the entry would otherwise leak across turns — reset here so
		// the map can't grow unbounded.
		this.editSnapshots.clear();

		// Extract text from prompt parts
		const promptParts = params.prompt ?? [];
		const text = promptParts
			.map((p: { type?: string; text?: string }) =>
				p.type === "text" ? (p.text ?? "") : "",
			)
			.join("");

		// session.prompt() resolves when the agent loop completes.
		// Events fire via subscribe() during execution and are translated
		// to ACP notifications in handlePiEvent().
		try {
			await session.prompt(text);
		} catch (error) {
			if (!this.cancelRequested) {
				throw error;
			}
		}

		if (!this.emittedAssistantText) {
			const latestText = this.latestAssistantText();
			await this.emitAssistantText(latestText);
		}

		// The SDK resolves prompt() before its queued session event pipeline
		// has necessarily drained through subscribe() listeners.
		await new Promise<void>((resolve) => setTimeout(resolve, 0));

		await this.flushPendingUpdates();
		await this.lastEmit;

		const stopReason = this.cancelRequested ? "cancelled" : "end_turn";
		return {
			stopReason: stopReason as PromptResponse["stopReason"],
		};
	}

	async cancel(_params: CancelNotification): Promise<void> {
		this.cancelRequested = true;
		// A cancelled turn may abort tools mid-execution before their
		// tool_execution_end fires; clear the per-tool maps so their entries don't
		// leak (prompt() also resets these at the next turn's start).
		this.currentToolCalls.clear();
		this.editSnapshots.clear();
		await this.session?.abort();
	}

	async setSessionMode(
		params: SetSessionModeRequest,
	): Promise<SetSessionModeResponse | void> {
		if (!this.session) return;

		this.session.setThinkingLevel(
			params.modeId as Parameters<PiSessionLike["setThinkingLevel"]>[0],
		);

		await this.emit({
			sessionUpdate: "current_mode_update" as const,
			currentModeId: params.modeId,
		});
	}

	async authenticate(
		_params: AuthenticateRequest,
	): Promise<AuthenticateResponse | void> {
		// Auth handled via env vars (ANTHROPIC_API_KEY)
	}

	// ── Event translation ───────────────────────────────────────────

	private emit(update: SessionNotification["update"]): Promise<void> {
		if (this.bufferingUpdates) {
			this.pendingUpdates.push(update);
			return Promise.resolve();
		}
		return this.sendUpdate(update);
	}

	private sendUpdate(update: SessionNotification["update"]): Promise<void> {
		this.lastEmit = this.lastEmit
			.then(() =>
				this.conn.sessionUpdate({
					sessionId: this.sessionId,
					update,
				}),
			)
			// The catch is load-bearing: `lastEmit` is awaited at turn end and a
			// rejected chain would halt all later updates and surface as a spurious
			// prompt failure (plus an unhandled rejection on detached emit callers).
			// But never swallow silently — a dropped session/update (host disconnect
			// / broken pipe) must be host-visible, so log it to stderr (the
			// onAgentStderr channel).
			.catch((error) => {
				console.warn(
					`[pi-sdk-acp] failed to deliver session/update: ${
						error instanceof Error ? error.message : String(error)
					}`,
				);
			});
		return this.lastEmit;
	}

	private async flushPendingUpdates(): Promise<void> {
		const updates = this.pendingUpdates;
		this.pendingUpdates = [];
		for (const update of updates) {
			await this.sendUpdate(update);
		}
	}

	private emitAssistantText(text: string): Promise<void> {
		if (!text) {
			return Promise.resolve();
		}
		this.emittedAssistantText = true;
		return this.emit({
			sessionUpdate: "agent_message_chunk",
			content: {
				type: "text",
				text,
			},
		});
	}

	private handlePiEvent(event: AgentSessionEvent): void {
		switch (event.type) {
			case "message_update": {
				const ame = event.assistantMessageEvent;
				if (!ame) break;

				if (ame.type === "text_delta" && "delta" in ame) {
					this.streamedTextContent.add(this.textContentKey(ame));
					this.emitAssistantText(String((ame as { delta: string }).delta));
				} else if (ame.type === "text_end" && "content" in ame) {
					const textKey = this.textContentKey(ame);
					if (!this.streamedTextContent.has(textKey)) {
						this.emitAssistantText(String((ame as { content: string }).content));
					}
				} else if (ame.type === "thinking_delta" && "delta" in ame) {
					this.emit({
						sessionUpdate: "agent_thought_chunk",
						content: {
							type: "text",
							text: String((ame as { delta: string }).delta),
						},
					});
				} else if (
					ame.type === "toolcall_start" ||
					ame.type === "toolcall_delta" ||
					ame.type === "toolcall_end"
				) {
					this.handleToolCallMessage(ame);
				}
				break;
			}

			case "tool_execution_start":
				this.handleToolExecutionStart(event);
				break;

			case "tool_execution_update":
				this.handleToolExecutionUpdate(event);
				break;

			case "tool_execution_end":
				this.handleToolExecutionEnd(event);
				break;

			case "agent_end":
				// Agent loop finished. Notifications are flushed in prompt().
				break;
		}
	}

	private handleToolCallMessage(ame: Record<string, unknown>): void {
		const toolCall =
			(ame.toolCall as Record<string, unknown>) ??
			(
				(ame.partial as Record<string, unknown>)
					?.content as Array<Record<string, unknown>>
			)?.[(ame.contentIndex as number) ?? 0];

		if (!toolCall) return;

		const toolCallId = String(toolCall.id ?? "");
		const toolName = String(toolCall.name ?? "tool");

		if (!toolCallId) return;

		const rawInput = this.parseToolArgs(toolCall);
		const locations = this.toToolCallLocations(rawInput);
		const existingStatus = this.currentToolCalls.get(toolCallId);
		const status = existingStatus ?? "pending";

		if (!existingStatus) {
			this.currentToolCalls.set(toolCallId, "pending");
			this.emit({
				sessionUpdate: "tool_call",
				toolCallId,
				title: toolName,
				kind: toToolKind(toolName),
				status: status as "pending",
				locations,
				rawInput,
			});
		} else {
			this.emit({
				sessionUpdate: "tool_call_update",
				toolCallId,
				status: status as "pending",
				locations,
				rawInput,
			});
		}
	}

	private handleToolExecutionStart(event: {
		toolCallId: string;
		toolName: string;
		args: unknown;
	}): void {
		const { toolCallId, toolName, args } = event;
		const rawInput = args as Record<string, unknown> | undefined;

		// Snapshot for edit diff support
		if (toolName === "edit" && rawInput) {
			const p =
				typeof rawInput.path === "string" ? rawInput.path : undefined;
			if (p) {
				try {
					const abs = isAbsolute(p)
						? p
						: resolvePath(this.cwd, p);
					const oldText = readFileSync(abs, "utf8");
					this.editSnapshots.set(toolCallId, {
						path: p,
						oldText,
					});
				} catch {
					// File may not exist
				}
			}
		}

		const locations = this.toToolCallLocations(rawInput);

		if (!this.currentToolCalls.has(toolCallId)) {
			this.currentToolCalls.set(toolCallId, "in_progress");
			this.emit({
				sessionUpdate: "tool_call",
				toolCallId,
				title: toolName,
				kind: toToolKind(toolName),
				status: "in_progress",
				locations,
				rawInput,
			});
		} else {
			this.currentToolCalls.set(toolCallId, "in_progress");
			this.emit({
				sessionUpdate: "tool_call_update",
				toolCallId,
				status: "in_progress",
				locations,
				rawInput,
			});
		}
	}

	private handleToolExecutionUpdate(event: {
		toolCallId: string;
		partialResult: unknown;
	}): void {
		const { toolCallId, partialResult } = event;
		const text = toolResultToText(partialResult);

		this.emit({
			sessionUpdate: "tool_call_update",
			toolCallId,
			status: "in_progress",
			content: text
				? [{ type: "content", content: { type: "text", text } }]
				: undefined,
			rawOutput: partialResult as Record<string, unknown>,
		});
	}

	private handleToolExecutionEnd(event: {
		toolCallId: string;
		result: unknown;
		isError: boolean;
	}): void {
		const { toolCallId, result, isError } = event;
		const text = toolResultToText(result);
		const snapshot = this.editSnapshots.get(toolCallId);

		let content:
			| Array<
					| { type: "diff"; path: string; oldText: string; newText: string }
					| { type: "content"; content: { type: "text"; text: string } }
				>
			| undefined;

		// Generate diff for edit tool
		if (!isError && snapshot) {
			try {
				const abs = isAbsolute(snapshot.path)
					? snapshot.path
					: resolvePath(this.cwd, snapshot.path);
				const newText = readFileSync(abs, "utf8");
				if (newText !== snapshot.oldText) {
					content = [
						{
							type: "diff" as const,
							path: snapshot.path,
							oldText: snapshot.oldText,
							newText,
						},
						...(text
							? [
									{
										type: "content" as const,
										content: { type: "text" as const, text },
									},
								]
							: []),
					];
				}
			} catch {
				// File may have been deleted
			}
		}

		if (!content && text) {
			content = [
				{ type: "content" as const, content: { type: "text" as const, text } },
			];
		}

		this.emit({
			sessionUpdate: "tool_call_update",
			toolCallId,
			status: isError ? "failed" : "completed",
			content,
			rawOutput: result as Record<string, unknown>,
		});

		this.currentToolCalls.delete(toolCallId);
		this.editSnapshots.delete(toolCallId);
	}

	// ── Helpers ──────────────────────────────────────────────────────

	private parseToolArgs(
		toolCall: Record<string, unknown>,
	): Record<string, unknown> | undefined {
		if (
			toolCall.arguments &&
			typeof toolCall.arguments === "object"
		) {
			return toolCall.arguments as Record<string, unknown>;
		}
		const s = String(toolCall.partialArgs ?? "");
		if (!s) return undefined;
		try {
			return JSON.parse(s);
		} catch {
			return { partialArgs: s };
		}
	}

	private toToolCallLocations(
		args: Record<string, unknown> | undefined,
	): Array<{ path: string; line?: number }> | undefined {
		const path =
			typeof args?.path === "string" ? args.path : undefined;
		if (!path) return undefined;
		const resolvedPath = isAbsolute(path)
			? path
			: resolvePath(this.cwd, path);
		return [{ path: resolvedPath }];
	}

	private textContentKey(ame: Record<string, unknown>): string {
		const contentIndex =
			typeof ame.contentIndex === "number" ? ame.contentIndex : -1;
		return String(contentIndex);
	}

	private latestAssistantText(): string {
		if (!this.session) {
			return "";
		}

		for (let index = this.session.messages.length - 1; index >= 0; index--) {
			const message = this.session.messages[index] as {
				role?: string;
				content?: unknown;
			};
			if (message.role !== "assistant") {
				continue;
			}

			const content = message.content;
			if (typeof content === "string") {
				return content;
			}
			if (!Array.isArray(content)) {
				const errorMessage =
					typeof (message as { errorMessage?: unknown }).errorMessage === "string"
						? (message as { errorMessage: string }).errorMessage
						: "";
				return errorMessage;
			}

			const text = content
				.map((part) => {
					const block = part as { type?: string; text?: string };
					return block.type === "text" && typeof block.text === "string"
						? block.text
						: "";
				})
				.filter(Boolean)
				.join("");
			if (text) {
				return text;
			}

			const errorMessage =
				typeof (message as { errorMessage?: unknown }).errorMessage === "string"
					? (message as { errorMessage: string }).errorMessage
					: "";
			return errorMessage;
		}

		return "";
	}

	private wrapTools(tools: PiToolLike[]): PiToolLike[] {
		return tools.map((tool) => ({
			...tool,
			execute: async (toolCallId, args, signal, onUpdate) => {
				const rawInput =
					args && typeof args === "object"
						? (args as Record<string, unknown>)
						: undefined;
				const locations = this.toToolCallLocations(rawInput);

				this.currentToolCalls.set(toolCallId, "in_progress");
				await this.emit({
					sessionUpdate: "tool_call",
					toolCallId,
					title: tool.name,
					kind: toToolKind(tool.name),
					status: "in_progress",
					locations,
					rawInput,
				});

				try {
					const result = await tool.execute(
						toolCallId,
						args,
						signal,
						(partialResult) => {
							void this.emit({
								sessionUpdate: "tool_call_update",
								toolCallId,
								status: "in_progress",
								content: toTextContent(toolResultToText(partialResult)),
								rawOutput:
									partialResult && typeof partialResult === "object"
										? (partialResult as Record<string, unknown>)
										: undefined,
							});
							onUpdate?.(partialResult);
						},
					);

					await this.emit({
						sessionUpdate: "tool_call_update",
						toolCallId,
						status: "completed",
						content: toTextContent(toolResultToText(result)),
						rawOutput:
							result && typeof result === "object"
								? (result as Record<string, unknown>)
								: undefined,
					});
					return result;
				} catch (error) {
					await this.emit({
						sessionUpdate: "tool_call_update",
						toolCallId,
						status: "failed",
						content:
							error instanceof Error
								? toTextContent(error.message)
								: undefined,
					});
					throw error;
				} finally {
					this.currentToolCalls.delete(toolCallId);
				}
			},
		}));
	}
}

// ── Standalone helpers ──────────────────────────────────────────────

function toToolKind(
	toolName: string,
): "read" | "edit" | "other" {
	if (toolName === "read") return "read";
	if (toolName === "write" || toolName === "edit") return "edit";
	return "other";
}

function toTextContent(text: string):
	| Array<{ type: "content"; content: { type: "text"; text: string } }>
	| undefined {
	if (!text) {
		return undefined;
	}
	return [
		{
			type: "content",
			content: {
				type: "text",
				text,
			},
		},
	];
}

function toolResultToText(result: unknown): string {
	if (!result) return "";
	const r = result as Record<string, unknown>;
	const content = r.content;
	if (Array.isArray(content)) {
		const texts = content
			.map((c: Record<string, unknown>) =>
				c?.type === "text" && typeof c.text === "string"
					? c.text
					: "",
			)
			.filter(Boolean);
		if (texts.length) return texts.join("");
	}
	const details = r.details as Record<string, unknown> | undefined;
	const stdout =
		(typeof details?.stdout === "string" ? details.stdout : undefined) ??
		(typeof r.stdout === "string" ? r.stdout : undefined) ??
		(typeof details?.output === "string" ? details.output : undefined) ??
		(typeof r.output === "string" ? r.output : undefined);
	const stderr =
		(typeof details?.stderr === "string" ? details.stderr : undefined) ??
		(typeof r.stderr === "string" ? r.stderr : undefined);
	const exitCode =
		(typeof details?.exitCode === "number"
			? details.exitCode
			: undefined) ??
		(typeof r.exitCode === "number" ? r.exitCode : undefined) ??
		(typeof details?.code === "number" ? details.code : undefined) ??
		(typeof r.code === "number" ? r.code : undefined);

	if (
		(typeof stdout === "string" && stdout.trim()) ||
		(typeof stderr === "string" && stderr.trim())
	) {
		const parts: string[] = [];
		if (typeof stdout === "string" && stdout.trim()) parts.push(stdout);
		if (typeof stderr === "string" && stderr.trim())
			parts.push(`stderr:\n${stderr}`);
		if (typeof exitCode === "number")
			parts.push(`exit code: ${exitCode}`);
		return parts.join("\n\n").trimEnd();
	}

	try {
		return JSON.stringify(result, null, 2);
	} catch {
		return String(result);
	}
}

// ── Entry point ─────────────────────────────────────────────────────

const input = new WritableStream<Uint8Array>({
	write(chunk) {
		return new Promise<void>((resolve) => {
			process.stdout.write(chunk, () => resolve());
		});
	},
});

const output = new ReadableStream<Uint8Array>({
	start(controller) {
		realStdin.on("data", (chunk: Buffer) => {
			controller.enqueue(new Uint8Array(chunk));
		});
		realStdin.on("end", () => controller.close());
		realStdin.on("error", (error: Error) => controller.error(error));
	},
});

const stream = ndJsonStream(input, output);
const _connection = new AgentSideConnection(
	(conn) => new PiSdkAgent(conn),
	stream,
);

// Keep process alive
realStdin.resume();

// Shutdown on stdin close
realStdin.on("end", () => {
	process.exit(0);
});
