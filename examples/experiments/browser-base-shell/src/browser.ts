import { Buffer as BufferPolyfill } from "buffer";

(globalThis as unknown as { Buffer?: unknown }).Buffer ??= BufferPolyfill;

import { createAgentOsConvergedSidecar } from "@rivet-dev/agentos-browser";
import {
	allowAll,
	type CommandExecutor,
	createBrowserDriver,
	createBrowserNetworkAdapter,
	createBrowserRuntimeDriverFactory,
	createWasiCommandBootstrapScript,
	type NodeRuntimeDriver,
	type ProcessConfig,
	type PtyOpenResult,
} from "@rivet-dev/agentos-runtime-browser";

const OUTPUT_BINDING = "__agentOsPtyOutput";
const CLAUDE_COMMAND = "claude";
const CLAUDE_FILE =
	"/opt/agentos/pkgs/claude/0.2.1/claude-cli-patched-afacb412a5c49a09.mjs";
const BROWSER_SHELL_PACKAGE_ROOT = "/opt/agentos/pkgs/browser-base-shell/0.0.1";
const WASM_COMMANDS = [
	"bash",
	"cat",
	"chmod",
	"echo",
	"git",
	"git-remote-http",
	"git-remote-https",
	"ls",
	"mkdir",
	"pwd",
	"printf",
	"sh",
	"tr",
	"true",
	"vim",
];
const SHELL_ENV = {
	HOME: "/root",
	PATH: "/opt/agentos/bin:/bin:/usr/bin",
	SHELL: "/bin/sh",
	TERM: "xterm-256color",
	VIM: "/usr/local/share/vim",
	VIMRUNTIME: "/usr/local/share/vim/vim92",
};
const CLAUDE_ENV = {
	CLAUDE_AGENT_SDK_CLIENT_APP: "@rivet-dev/agentos",
	CLAUDE_CODE_SIMPLE: "1",
	CLAUDE_CODE_FORCE_AGENT_OS_RIPGREP: "1",
	CLAUDE_CODE_DEFER_GROWTHBOOK_INIT: "1",
	CLAUDE_CODE_DISABLE_CWD_PERSIST: "1",
	CLAUDE_CODE_DISABLE_DEV_NULL_REDIRECT: "1",
	CLAUDE_CODE_NODE_SHELL_WRAPPER: "1",
	CLAUDE_CODE_DISABLE_STREAM_JSON_HOOK_EVENTS: "1",
	CLAUDE_CODE_SHELL: "/bin/sh",
	CLAUDE_CODE_SKIP_INITIAL_MESSAGES: "1",
	CLAUDE_CODE_SKIP_SANDBOX_INIT: "1",
	CLAUDE_CODE_SIMPLE_SHELL_EXEC: "1",
	CLAUDE_CODE_SWAP_STDIO: "0",
	CLAUDE_CODE_USE_PIPE_OUTPUT: "1",
	DISABLE_TELEMETRY: "1",
	HOME: "/root",
	PATH: "/opt/agentos/bin:/bin:/usr/bin",
	SHELL: "/bin/sh",
	TERM: "xterm-256color",
	USE_BUILTIN_RIPGREP: "0",
};
const config = {
	rootFilesystem: {
		mode: "ephemeral",
		disableDefaultBaseLayer: false,
		lowers: [],
		bootstrapEntries: [],
	},
	permissions: {
		fs: "allow",
		network: "allow",
		childProcess: "allow",
		process: "allow",
		env: "allow",
		binding: "allow",
	},
} as never;

interface ProcessPty {
	executionId: string;
	masterFd: number;
	slaveFd: number;
	running: boolean;
}

interface BridgeState {
	mode: "shell";
	crossOriginIsolated: boolean;
	shell?: ProcessPty;
	claudeSpawns: number;
	claudeLastExitCode: number | null;
	claudeLastError: string | null;
	claudeOutputBytes: number;
}

declare global {
	interface Window {
		__agentOsBrowserbase?: {
			start(): Promise<BridgeState>;
			writeBase64(data: string): Promise<void>;
			state(): BridgeState;
			dispose(): Promise<void>;
		};
	}
}

let shellDriver: NodeRuntimeDriver | undefined;
let shell: ProcessPty | undefined;
let started: Promise<BridgeState> | undefined;
let inputQueue = Promise.resolve();
let sequence = 0;
let claudeSpawns = 0;
let claudeLastExitCode: number | null = null;
let claudeLastError: string | null = null;
let claudeOutputBytes = 0;
let disposing = false;
const childDrivers = new Set<NodeRuntimeDriver>();
const encoder = new TextEncoder();

function status(value: string): void {
	const element = document.getElementById("status");
	if (element) element.textContent = value;
}

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function bytesToBase64(bytes: Uint8Array): string {
	let binary = "";
	for (const byte of bytes) binary += String.fromCharCode(byte);
	return btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
	const binary = atob(value);
	return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

function emit(
	kind: "pty" | "status" | "error",
	data: string | Uint8Array,
): void {
	const binding = (globalThis as unknown as Record<string, unknown>)[
		OUTPUT_BINDING
	];
	if (typeof binding !== "function") {
		throw new Error(`CDP binding ${OUTPUT_BINDING} is not installed`);
	}
	const bytes = typeof data === "string" ? encoder.encode(data) : data;
	(binding as (payload: string) => void)(
		JSON.stringify({
			sequence: sequence++,
			kind,
			mode: "shell",
			base64: bytesToBase64(bytes),
		}),
	);
}

function state(): BridgeState {
	return {
		mode: "shell",
		crossOriginIsolated: globalThis.crossOriginIsolated,
		...(shell ? { shell: { ...shell } } : {}),
		claudeSpawns,
		claudeLastExitCode,
		claudeLastError,
		claudeOutputBytes,
	};
}

async function waitForExecutionId(getId: () => string, timeoutMs = 5_000) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const id = getId();
		if (id) return id;
		await delay(0);
	}
	throw new Error("timed out waiting for AgentOS execution id");
}

async function createRuntimeDriver(
	options: { commandExecutor?: CommandExecutor; process?: ProcessConfig } = {},
): Promise<NodeRuntimeDriver> {
	const system = await createBrowserDriver({
		filesystem: "memory",
		permissions: allowAll,
		networkAdapter: createBrowserNetworkAdapter(),
	});
	if (options.commandExecutor) system.commandExecutor = options.commandExecutor;
	(system as { runtime?: unknown }).runtime = {
		process: options.process ?? {},
		os: {},
	};
	const factory = createBrowserRuntimeDriverFactory({
		workerUrl: new URL("/agentos-worker.js", window.location.href),
		convergedSidecar: createAgentOsConvergedSidecar(config, {
			moduleUrl: "/wasm/agentos_sidecar_browser.js",
			binaryUrl: "/wasm/agentos_sidecar_browser_bg.wasm",
		}),
	});
	return factory.createRuntimeDriver({
		system,
		runtime: (system as { runtime: { process: unknown; os: unknown } }).runtime,
	} as never);
}

async function loadClaudeSource(): Promise<string> {
	const response = await fetch("/claude-cli.mjs");
	if (!response.ok) {
		throw new Error(
			`failed to load AgentOS Claude package: HTTP ${response.status}`,
		);
	}
	return response.text();
}

function createClaudeCommandExecutor(claudeSource: string): CommandExecutor {
	return {
		spawn(command, args, options) {
			const commandName = command.split("/").filter(Boolean).at(-1) ?? command;
			let driver: NodeRuntimeDriver | undefined;
			let child: ProcessPty | undefined;
			let closed = false;
			const pendingInput: Uint8Array[] = [];

			const run = (async () => {
				if (commandName !== CLAUDE_COMMAND) {
					options?.onStderr?.(
						encoder.encode(`${commandName}: command not found\n`),
					);
					return 127;
				}
				claudeSpawns += 1;
				claudeLastExitCode = null;
				claudeLastError = null;
				claudeOutputBytes = 0;
				driver = await createRuntimeDriver({
					process: {
						argv: ["node", CLAUDE_FILE, ...args],
						cwd: options?.cwd ?? "/",
						env: { ...CLAUDE_ENV, ...options?.env },
					},
				});
				childDrivers.add(driver);
				let executionId = "";
				let resolvePty!: (opened: PtyOpenResult) => void;
				const ptyPromise = new Promise<PtyOpenResult>((resolve) => {
					resolvePty = resolve;
				});
				let completed = false;
				const resultPromise = driver
					.exec(claudeSource, {
						filePath: CLAUDE_FILE,
						persistent: true,
						timingMitigation: "off",
						cwd: options?.cwd ?? "/",
						env: { ...CLAUDE_ENV, ...options?.env },
						onStart: (id) => {
							executionId = id;
						},
						stdioPty: {
							open: true,
							columns: 100,
							rows: 32,
							onOpen: resolvePty,
						},
					})
					.finally(() => {
						completed = true;
					});
				const opened = await ptyPromise;
				executionId = await waitForExecutionId(() => executionId);
				child = {
					executionId,
					masterFd: opened.masterFd,
					slaveFd: opened.slaveFd,
					running: true,
				};
				for (const bytes of pendingInput.splice(0)) {
					await driver.writePty?.(executionId, opened.masterFd, bytes);
				}
				const pump = (async () => {
					let emptyReadsAfterExit = 0;
					while (child?.running && (!completed || emptyReadsAfterExit < 3)) {
						const bytes = await driver?.readPty?.(
							executionId,
							opened.masterFd,
							{
								timeoutMs: 0,
								maxBytes: 8_192,
							},
						);
						if (bytes?.byteLength) {
							emptyReadsAfterExit = 0;
							claudeOutputBytes += bytes.byteLength;
							options?.onStdout?.(bytes);
						} else {
							if (completed) emptyReadsAfterExit += 1;
							await delay(5);
						}
					}
				})();
				const result = await resultPromise;
				await pump;
				child.running = false;
				claudeLastExitCode = result.code;
				claudeLastError = result.errorMessage ?? null;
				if (result.errorMessage) {
					options?.onStderr?.(encoder.encode(`${result.errorMessage}\n`));
				}
				return result.code;
			})()
				.catch((error: unknown) => {
					claudeLastError =
						error instanceof Error
							? (error.stack ?? error.message)
							: String(error);
					console.error("AgentOS Claude browser execution failed", error);
					options?.onStderr?.(encoder.encode(`${claudeLastError}\n`));
					claudeLastExitCode = 1;
					return 1;
				})
				.finally(() => {
					if (child) child.running = false;
					if (driver) {
						childDrivers.delete(driver);
						driver.dispose();
					}
				});

			return {
				wait: () => run,
				writeStdin(data) {
					const bytes = typeof data === "string" ? encoder.encode(data) : data;
					if (!driver || !child) pendingInput.push(bytes);
					else void driver.writePty?.(child.executionId, child.masterFd, bytes);
				},
				closeStdin() {
					if (closed) return;
					closed = true;
					const eof = Uint8Array.of(4);
					if (!driver || !child) pendingInput.push(eof);
					else void driver.writePty?.(child.executionId, child.masterFd, eof);
				},
				kill() {
					if (child) child.running = false;
					driver?.dispose();
				},
			};
		},
	};
}

function shellBootstrap(): string {
	const commandUrl = (name: string) =>
		new URL(`/commands/${name}`, window.location.href).href;
	const commandGuestPath = (name: string) =>
		`${BROWSER_SHELL_PACKAGE_ROOT}/bin/${name}`;
	const pathSetup = `
const __fs = require("node:fs");
__fs.mkdirSync("/opt/agentos/bin", { recursive: true });
__fs.mkdirSync("/bin", { recursive: true });
__fs.mkdirSync("/usr/bin", { recursive: true });
for (const __name of ${JSON.stringify(WASM_COMMANDS)}) {
	for (const __path of ["/opt/agentos/bin/" + __name, "/bin/" + __name]) {
		try { __fs.unlinkSync(__path); } catch (__error) {
			if (__error && __error.code !== "ENOENT") throw __error;
		}
		__fs.symlinkSync(${JSON.stringify(`${BROWSER_SHELL_PACKAGE_ROOT}/bin/`)} + __name, __path);
	}
}
const __claudePath = "/usr/bin/${CLAUDE_COMMAND}";
__fs.writeFileSync(__claudePath, "");
__fs.chmodSync(__claudePath, 0o755);
`;
	return (
		pathSetup +
		createWasiCommandBootstrapScript({
			commandSource: commandUrl("sh"),
			command: "sh",
			args: ["--input-backend", "minimal"],
			commandFiles: Object.fromEntries(
				WASM_COMMANDS.map((name) => [commandGuestPath(name), commandUrl(name)]),
			),
			externalCommands: [CLAUDE_COMMAND],
			env: SHELL_ENV,
			cwd: "/",
			preopens: { "/": "/" },
			bootMessage: "AGENTOS_BROWSERBASE_SHELL",
			errorMessagePrefix: "AGENTOS_BROWSERBASE_SHELL_ERROR:",
		})
	);
}

async function startShell(claudeSource: string): Promise<void> {
	if (shellDriver || shell) return;
	shellDriver = await createRuntimeDriver({
		commandExecutor: createClaudeCommandExecutor(claudeSource),
	});
	let executionId = "";
	let resolvePty!: (opened: PtyOpenResult) => void;
	const ptyPromise = new Promise<PtyOpenResult>((resolve) => {
		resolvePty = resolve;
	});
	void shellDriver
		.exec(shellBootstrap(), {
			filePath: "/browserbase-shell.cjs",
			persistent: true,
			timingMitigation: "off",
			onStart: (id) => {
				executionId = id;
			},
			stdioPty: { open: true, columns: 100, rows: 32, onOpen: resolvePty },
		})
		.catch((error: unknown) => {
			if (disposing) return;
			emit(
				"error",
				error instanceof Error ? (error.stack ?? error.message) : String(error),
			);
		});
	const opened = await ptyPromise;
	executionId = await waitForExecutionId(() => executionId);
	shell = {
		executionId,
		masterFd: opened.masterFd,
		slaveFd: opened.slaveFd,
		running: true,
	};
	void (async () => {
		while (shell?.running && shellDriver) {
			const bytes = await shellDriver.readPty?.(executionId, opened.masterFd, {
				timeoutMs: 0,
				maxBytes: 8_192,
			});
			if (bytes?.byteLength) emit("pty", bytes);
			else await delay(10);
		}
	})().catch((error: unknown) => {
		if (disposing) return;
		emit(
			"error",
			error instanceof Error ? (error.stack ?? error.message) : String(error),
		);
	});
}

async function write(bytes: Uint8Array): Promise<void> {
	if (!shellDriver || !shell?.running) {
		throw new Error("AgentOS browser shell PTY is not running");
	}
	await shellDriver.writePty?.(shell.executionId, shell.masterFd, bytes);
}

async function start(): Promise<BridgeState> {
	if (started !== undefined) return started;
	started = (async () => {
		if (!globalThis.crossOriginIsolated) {
			throw new Error(
				"AgentOS browser VM requires COOP/COEP cross-origin isolation",
			);
		}
		status("Loading native AgentOS software");
		const claudeSource = await loadClaudeSource();
		status("Starting AgentOS browser VM over CDP");
		await startShell(claudeSource);
		status("AgentOS sh, Vim, and Claude over CDP");
		emit("status", "SHELL_PTY_READY\n");
		return state();
	})();
	return started;
}

async function dispose(): Promise<void> {
	disposing = true;
	if (shell?.running && shellDriver) {
		shell.running = false;
		await shellDriver
			.closePty?.(shell.executionId, shell.masterFd)
			.catch((error) => {
				console.error("failed to close AgentOS browser PTY", error);
			});
	}
	for (const driver of childDrivers) driver.dispose();
	childDrivers.clear();
	shellDriver?.dispose();
}

window.__agentOsBrowserbase = {
	start,
	writeBase64: (data) => {
		inputQueue = inputQueue.then(() => write(base64ToBytes(data)));
		return inputQueue;
	},
	state,
	dispose,
};

status("AgentOS browser VM waiting for CDP");
