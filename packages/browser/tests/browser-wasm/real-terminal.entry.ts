import { Buffer as BufferPolyfill } from "buffer";

(globalThis as unknown as { Buffer?: unknown }).Buffer ??= BufferPolyfill;

import {
	allowAll,
	createBrowserDriver,
	createBrowserRuntimeDriverFactory,
	createWasiCommandBootstrapScript,
	type NodeRuntimeDriver,
	type PtyOpenResult,
} from "@rivet-dev/agentos-runtime-browser";
import { Terminal } from "@xterm/xterm";
import { createAgentOsConvergedSidecar } from "../../src/converged-sidecar.js";

const WASM_MODULE_URL = "/wasm/agentos_sidecar_browser.js";
const WASM_BINARY_URL = "/wasm/agentos_sidecar_browser_bg.wasm";

const SHELL_ENV = {
	HOME: "/root",
	PATH: "/opt/agentos/bin:/bin:/usr/bin",
	SHELL: "/bin/sh",
	TERM: "xterm-256color",
	VIM: "/usr/local/share/vim",
	VIMRUNTIME: "/usr/local/share/vim/vim92",
};

const BROWSER_TERMINAL_PACKAGE_ROOT =
	"/opt/agentos/pkgs/browser-terminal/0.0.1";

const FULL_SHELL_COMMANDS = [
	"bash",
	"cat",
	"chmod",
	"cp",
	"echo",
	"git",
	"git-remote-http",
	"git-remote-https",
	"ls",
	"mkdir",
	"mv",
	"printf",
	"pwd",
	"rm",
	"sh",
	"touch",
	"tr",
	"true",
	"vim",
];

// The package-level gate only needs sh + echo to prove the xterm -> kernel PTY
// path. Runnable demos opt into the full projected software set explicitly.
const SHELL_COMMANDS =
	window.__agentOSTerminalConfig?.software === "full"
		? FULL_SHELL_COMMANDS
		: ["echo", "sh"];

const COMMAND_LINK_SCRIPT = `
const __fs = require("node:fs");
for (const __directory of ["/opt/agentos/bin", "/bin", "/usr/bin"]) {
	__fs.mkdirSync(__directory, { recursive: true });
}
for (const __name of ${JSON.stringify(SHELL_COMMANDS)}) {
	const __target = ${JSON.stringify(`${BROWSER_TERMINAL_PACKAGE_ROOT}/bin/`)} + __name;
	for (const __directory of ["/opt/agentos/bin", "/bin", "/usr/bin"]) {
		const __path = __directory + "/" + __name;
		try { __fs.unlinkSync(__path); } catch (__error) {
			if (__error && __error.code !== "ENOENT") throw __error;
		}
		__fs.symlinkSync(__target, __path);
	}
}
`;

const GUEST =
	COMMAND_LINK_SCRIPT +
	createWasiCommandBootstrapScript({
		commandSource: "/commands/sh",
		command: "sh",
		// The browser PTY does not yet consume xterm cursor-position responses
		// without echoing them. Brush's minimal interactive backend avoids those
		// DSR probes and preserves completed output in xterm.
		args: ["--input-backend", "minimal"],
		commandFiles: Object.fromEntries(
			SHELL_COMMANDS.map((name) => [
				`${BROWSER_TERMINAL_PACKAGE_ROOT}/bin/${name}`,
				name === "bash" ? "/commands/sh" : `/commands/${name}`,
			]),
		),
		env: SHELL_ENV,
		cwd: "/",
		preopens: { "/": "/" },
		bootMessage: "REAL_TERMINAL_BOOT",
		errorMessagePrefix: "REAL_TERMINAL_ERROR:",
	});

declare global {
	interface Window {
		__agentOSTerminalConfig?: {
			software: "smoke" | "full";
		};
		__realTerminal?: {
			start(): Promise<{ masterFd: number; slaveFd: number }>;
			write(data: string): Promise<void>;
			screen(): string;
			output(): string;
			dispose(): Promise<void>;
		};
	}
}

const terminalElement = document.getElementById("terminal");
const statusElement = document.getElementById("status");
if (!terminalElement) {
	throw new Error("missing #terminal");
}

const terminal = new Terminal({
	cols: 100,
	rows: 31,
	convertEol: true,
	cursorBlink: true,
	fontFamily:
		"ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, Liberation Mono, monospace",
	fontSize: 13,
	theme: {
		background: "#101214",
		foreground: "#e8edf2",
		cursor: "#7dd3fc",
		selectionBackground: "#334155",
	},
});
terminal.open(terminalElement);

let driver: NodeRuntimeDriver | undefined;
let executionId = "";
let pty: PtyOpenResult | undefined;
let pumpRunning = false;
let started: Promise<{ masterFd: number; slaveFd: number }> | undefined;
let disposing = false;
let runtimeError = "";
const decoder = new TextDecoder();
let output = "";

function setStatus(value: string): void {
	if (statusElement) statusElement.textContent = value;
}

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function formatError(error: unknown): string {
	return error instanceof Error ? error.stack || error.message : String(error);
}

function writeTerminal(data: string): Promise<void> {
	return new Promise((resolve) => terminal.write(data, resolve));
}

function failRuntime(error: unknown): void {
	if (disposing || runtimeError) return;
	runtimeError = formatError(error);
	setStatus("error");
	void writeTerminal(`REAL_TERMINAL_UI_ERROR:${runtimeError}\r\n`).catch(
		(renderError: unknown) => {
			console.error("failed to render browser-local shell error", renderError);
		},
	);
}

async function waitForExecutionId(timeoutMs = 5_000): Promise<string> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (executionId) return executionId;
		await delay(0);
	}
	throw new Error("timed out waiting for execution id");
}

async function pumpPty(): Promise<void> {
	if (!driver || !pty) return;
	pumpRunning = true;
	try {
		while (pumpRunning && driver && pty && executionId) {
			const bytes = await driver.readPty!(executionId, pty.masterFd, {
				timeoutMs: 10,
				maxBytes: 4096,
			});
			if (bytes?.byteLength) {
				const text = decoder.decode(bytes);
				await writeTerminal(text);
				output += text;
			} else {
				await delay(10);
			}
		}
	} catch (error) {
		failRuntime(error);
	}
}

async function waitForOutput(
	pattern: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (!output.includes(pattern)) {
		if (runtimeError) {
			throw new Error(
				`browser-local shell failed before ${JSON.stringify(pattern)}: ${runtimeError}; output=${JSON.stringify(output)}`,
			);
		}
		if (Date.now() >= deadline) {
			throw new Error(
				`timed out waiting for browser-local shell output ${JSON.stringify(pattern)}; output=${JSON.stringify(output)}`,
			);
		}
		await delay(10);
	}
}

function terminalScreen(): string {
	const buffer = terminal.buffer.active;
	const lines: string[] = [];
	for (let i = 0; i < buffer.length; i += 1) {
		lines.push(buffer.getLine(i)?.translateToString(true) ?? "");
	}
	return lines.join("\n");
}

async function start(): Promise<{ masterFd: number; slaveFd: number }> {
	if (started) return started;
	started = (async () => {
		setStatus("booting");
		const system = await createBrowserDriver({
			filesystem: "memory",
			permissions: allowAll,
			useDefaultNetwork: true,
		});
		(system as { runtime?: unknown }).runtime = { process: {}, os: {} };
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
		const factory = createBrowserRuntimeDriverFactory({
			workerUrl: new URL("/agentos-worker.js", window.location.href),
			convergedSidecar: createAgentOsConvergedSidecar(config, {
				moduleUrl: WASM_MODULE_URL,
				binaryUrl: WASM_BINARY_URL,
			}),
		});
		driver = factory.createRuntimeDriver({
			system,
			runtime: (system as { runtime: { process: unknown; os: unknown } })
				.runtime,
		} as never);

		let resolvePty!: (opened: PtyOpenResult) => void;
		let rejectPty!: (error: unknown) => void;
		const ptyPromise = new Promise<PtyOpenResult>((resolve, reject) => {
			resolvePty = resolve;
			rejectPty = reject;
		});
		const execPromise = driver.exec(GUEST, {
			filePath: "/r3-real-terminal-ui.js",
			persistent: true,
			timingMitigation: "off",
			onStart: (id) => {
				executionId = id;
			},
			stdioPty: {
				open: true,
				columns: 100,
				rows: 31,
				onOpen: resolvePty,
			},
		});
		void execPromise.then(
			(result) => {
				const error = new Error(
					`browser-local shell execution ended unexpectedly: ${JSON.stringify(result)}`,
				);
				rejectPty(error);
				failRuntime(error);
			},
			(error: unknown) => {
				rejectPty(error);
				failRuntime(error);
			},
		);
		pty = await ptyPromise;
		executionId = await waitForExecutionId();
		terminal.onData((data) => {
			void driver
				?.writePty?.(executionId, pty!.masterFd, data)
				.catch((error: unknown) => failRuntime(error));
		});
		void pumpPty();
		await waitForOutput("sh-0.4$ ", 30_000);
		terminal.focus();
		setStatus("running");
		return { masterFd: pty.masterFd, slaveFd: pty.slaveFd };
	})().catch((error) => {
		failRuntime(error);
		throw error;
	});
	return started;
}

async function dispose(): Promise<void> {
	disposing = true;
	pumpRunning = false;
	if (driver && pty && executionId) {
		await driver
			.closePty?.(executionId, pty.masterFd)
			.catch((error: unknown) => {
				console.error("failed to close browser-local shell PTY", error);
			});
	}
	driver?.dispose?.();
}

async function write(data: string): Promise<void> {
	await start();
	if (!driver || !pty || !executionId) {
		throw new Error("browser-local shell PTY is not running");
	}
	await driver.writePty?.(executionId, pty.masterFd, data);
}

window.__realTerminal = {
	start,
	write,
	screen: terminalScreen,
	output: () => output,
	dispose,
};

setStatus("ready");
