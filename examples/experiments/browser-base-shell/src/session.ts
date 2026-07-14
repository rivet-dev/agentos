import { type ChildProcess, spawn } from "node:child_process";
import { Resolver } from "node:dns/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";
import {
	type Browser,
	type CDPSession,
	chromium,
	type Page,
} from "playwright-core";
import { ensurePersistentTunnel } from "./tunnel.js";

const OUTPUT_BINDING = "__agentOsPtyOutput";
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MAX_PROCESS_LOG_BYTES = 64 * 1_024;
const MAX_TRANSCRIPT_BYTES = 4 * 1_024 * 1_024;
const DEFAULT_VITE_PORT = 4_178;

export interface BrowserVmState {
	mode: "shell";
	crossOriginIsolated: boolean;
	shell?: {
		executionId: string;
		masterFd: number;
		slaveFd: number;
		running: boolean;
	};
	claudeSpawns: number;
	claudeLastExitCode: number | null;
	claudeLastError: string | null;
	claudeOutputBytes: number;
}

export interface PtyEvent {
	sequence: number;
	kind: "pty" | "status" | "error";
	mode: "shell";
	bytes: Buffer;
}

export interface BrowserbaseShellOptions {
	onEvent?: (event: PtyEvent) => void;
	onStatus?: (message: string) => void;
}

interface Credentials {
	apiKey: string;
	projectId: string;
}

interface CreatedSession {
	id: string;
	connectUrl: string;
}

interface BindingPayload {
	sequence: number;
	kind: PtyEvent["kind"];
	mode: PtyEvent["mode"];
	base64: string;
}

function delay(ms: number): Promise<void> {
	return new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
}

function trimProcessLog(value: string): string {
	return value.length > MAX_PROCESS_LOG_BYTES
		? value.slice(-MAX_PROCESS_LOG_BYTES)
		: value;
}

function credentials(): Credentials {
	const apiKey =
		process.env.BROWSERBASE_API_KEY ?? process.env.BROWSER_BASE_API_KEY;
	const projectId =
		process.env.BROWSERBASE_PROJECT_ID ?? process.env.BROWSER_BASE_PROJECT_ID;
	if (!apiKey || !projectId) {
		throw new Error(
			"Browserbase credentials are missing; export BROWSERBASE_API_KEY and BROWSERBASE_PROJECT_ID before starting the demo",
		);
	}
	return { apiKey, projectId };
}

function vitePort(): number {
	const value = process.env.AGENTOS_BROWSERBASE_PORT;
	if (!value) return DEFAULT_VITE_PORT;
	const port = Number(value);
	if (!Number.isInteger(port) || port < 1 || port > 65_535) {
		throw new Error(
			`AGENTOS_BROWSERBASE_PORT must be an integer from 1 to 65535, received ${JSON.stringify(value)}`,
		);
	}
	return port;
}

async function waitForHttp(url: string, timeoutMs: number): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;
	while (Date.now() < deadline) {
		try {
			const response = await fetch(url, { signal: AbortSignal.timeout(5_000) });
			if (response.ok) return;
			lastError = new Error(`HTTP ${response.status}`);
		} catch (error) {
			lastError = error;
		}
		await delay(150);
	}
	throw new Error(`timed out waiting for ${url}`, { cause: lastError });
}

async function waitForVite(
	process: ReturnType<typeof processWithLogs>,
	url: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;
	while (Date.now() < deadline) {
		if (process.child.exitCode !== null) {
			throw new Error(
				`Vite exited with code ${process.child.exitCode} before startup:\n${process.logs()}`,
			);
		}
		try {
			const response = await fetch(url, { signal: AbortSignal.timeout(5_000) });
			if (response.ok) return;
			lastError = new Error(`HTTP ${response.status}`);
		} catch (error) {
			lastError = error;
		}
		await delay(150);
	}
	throw new Error(`timed out waiting for Vite at ${url}:\n${process.logs()}`, {
		cause: lastError,
	});
}

async function waitForPublicDns(
	hostname: string,
	timeoutMs: number,
): Promise<void> {
	// Avoid poisoning the host resolver's negative cache by asking for a Quick
	// Tunnel hostname before Cloudflare has published its generated DNS record.
	const resolver = new Resolver();
	resolver.setServers(["1.1.1.1"]);
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;
	while (Date.now() < deadline) {
		try {
			const addresses = await resolver.resolve4(hostname);
			if (addresses.length > 0) return;
		} catch (error) {
			lastError = error;
		}
		await delay(150);
	}
	throw new Error(`timed out waiting for public DNS for ${hostname}`, {
		cause: lastError,
	});
}

function processWithLogs(
	command: string,
	args: string[],
): { child: ChildProcess; logs: () => string } {
	const child = spawn(command, args, {
		cwd: packageRoot,
		env: process.env,
		detached: true,
		stdio: ["ignore", "pipe", "pipe"],
	});
	let output = "";
	for (const stream of [child.stdout, child.stderr]) {
		stream?.setEncoding("utf8");
		stream?.on("data", (chunk: string) => {
			output = trimProcessLog(output + chunk);
		});
	}
	return { child, logs: () => output };
}

async function stopProcess(
	child: ChildProcess | undefined,
	name: string,
): Promise<void> {
	if (!child?.pid || child.exitCode !== null) return;
	try {
		process.kill(-child.pid, "SIGTERM");
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ESRCH") {
			console.warn(`failed to terminate ${name}`, error);
		}
		return;
	}
	await Promise.race([
		new Promise<void>((resolveExit) => child.once("exit", () => resolveExit())),
		delay(2_000),
	]);
	if (child.exitCode === null) {
		try {
			process.kill(-child.pid, "SIGKILL");
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ESRCH") {
				console.warn(`failed to kill ${name}`, error);
			}
		}
	}
}

async function createBrowserbaseSession(
	creds: Credentials,
): Promise<CreatedSession> {
	const response = await fetch("https://api.browserbase.com/v1/sessions", {
		method: "POST",
		headers: {
			"x-bb-api-key": creds.apiKey,
			"content-type": "application/json",
		},
		body: JSON.stringify({
			projectId: creds.projectId,
			browserSettings: { viewport: { width: 1280, height: 800 } },
			userMetadata: { agentos_browser_base_shell: "true" },
		}),
		signal: AbortSignal.timeout(30_000),
	});
	if (!response.ok) {
		throw new Error(
			`Browserbase session creation failed with HTTP ${response.status}: ${await response.text()}`,
		);
	}
	const session = (await response.json()) as Partial<CreatedSession>;
	if (!session.id || !session.connectUrl) {
		throw new Error("Browserbase session response omitted id or connectUrl");
	}
	return { id: session.id, connectUrl: session.connectUrl };
}

async function releaseBrowserbaseSession(
	creds: Credentials,
	sessionId: string,
): Promise<void> {
	const response = await fetch(
		`https://api.browserbase.com/v1/sessions/${sessionId}`,
		{
			method: "POST",
			headers: {
				"x-bb-api-key": creds.apiKey,
				"content-type": "application/json",
			},
			body: JSON.stringify({ status: "REQUEST_RELEASE" }),
			signal: AbortSignal.timeout(30_000),
		},
	);
	if (!response.ok) {
		throw new Error(
			`Browserbase session release failed with HTTP ${response.status}: ${await response.text()}`,
		);
	}
}

export class BrowserbaseShellSession {
	readonly sessionId: string;
	readonly tunnelUrl: string;
	readonly sessionUrl: string;
	private readonly creds: Credentials;
	private readonly browser: Browser;
	private readonly page: Page;
	private readonly cdp: CDPSession;
	private readonly vite: ChildProcess;
	private readonly onEvent?: (event: PtyEvent) => void;
	private transcriptBuffer = Buffer.alloc(0);
	private closed = false;

	private constructor(args: {
		creds: Credentials;
		session: CreatedSession;
		tunnelUrl: string;
		browser: Browser;
		page: Page;
		cdp: CDPSession;
		vite: ChildProcess;
		onEvent?: (event: PtyEvent) => void;
	}) {
		this.creds = args.creds;
		this.sessionId = args.session.id;
		this.tunnelUrl = args.tunnelUrl;
		this.sessionUrl = `https://www.browserbase.com/sessions/${args.session.id}`;
		this.browser = args.browser;
		this.page = args.page;
		this.cdp = args.cdp;
		this.vite = args.vite;
		this.onEvent = args.onEvent;
	}

	static async open(
		options: BrowserbaseShellOptions = {},
	): Promise<BrowserbaseShellSession> {
		const report = options.onStatus ?? (() => undefined);
		const creds = credentials();
		const port = vitePort();
		report(`starting Vite on 127.0.0.1:${port}`);
		const viteProcess = processWithLogs("pnpm", [
			"exec",
			"vite",
			"--host",
			"127.0.0.1",
			"--port",
			String(port),
			"--strictPort",
		]);
		let created: CreatedSession | undefined;
		let browser: Browser | undefined;
		try {
			await waitForVite(viteProcess, `http://127.0.0.1:${port}`, 30_000);
			const tunnel = await ensurePersistentTunnel({
				port,
				onStatus: report,
				validate: async (url) => {
					await waitForPublicDns(new URL(url).hostname, 30_000);
					await waitForHttp(url, 30_000);
				},
			});
			const tunnelUrl = tunnel.url;
			report("creating Browserbase session");
			created = await createBrowserbaseSession(creds);
			report(`connecting to Browserbase session ${created.id} over CDP`);
			browser = await chromium.connectOverCDP(created.connectUrl, {
				timeout: 30_000,
			});
			const context = browser.contexts()[0] ?? (await browser.newContext());
			const page = context.pages()[0] ?? (await context.newPage());
			const cdp = await context.newCDPSession(page);
			await cdp.send("Runtime.enable");
			await cdp.send("Runtime.addBinding", { name: OUTPUT_BINDING });

			const shell = new BrowserbaseShellSession({
				creds,
				session: created,
				tunnelUrl,
				browser,
				page,
				cdp,
				vite: viteProcess.child,
				onEvent: options.onEvent,
			});
			cdp.on("Runtime.bindingCalled", (event) => shell.bindingCalled(event));
			page.on("console", (message) => {
				if (message.type() === "error") {
					console.error(`[Browserbase page] ${message.text()}`);
				}
			});
			page.on("pageerror", (error) => {
				console.error("[Browserbase page error]", error);
			});
			report(`loading browser VM from ${tunnelUrl}`);
			await page.goto(tunnelUrl, {
				waitUntil: "domcontentloaded",
				timeout: 60_000,
			});
			await shell.evaluate<BrowserVmState>(
				"window.__agentOsBrowserbase.start()",
			);
			await shell.waitFor("AGENTOS_BROWSERBASE_SHELL", 60_000);
			return shell;
		} catch (error) {
			if (browser) {
				await browser.close().catch((closeError) => {
					console.warn(
						"failed to close Browserbase CDP connection",
						closeError,
					);
				});
			}
			if (created) {
				await releaseBrowserbaseSession(creds, created.id).catch(
					(releaseError) => {
						console.warn("failed to release Browserbase session", releaseError);
					},
				);
			}
			await stopProcess(viteProcess.child, "Vite");
			throw error;
		}
	}

	private bindingCalled(event: { name: string; payload: string }): void {
		if (event.name !== OUTPUT_BINDING) return;
		try {
			const payload = JSON.parse(event.payload) as BindingPayload;
			const bytes = Buffer.from(payload.base64, "base64");
			const ptyEvent: PtyEvent = {
				sequence: payload.sequence,
				kind: payload.kind,
				mode: payload.mode,
				bytes,
			};
			if (payload.kind === "pty") {
				this.transcriptBuffer = Buffer.concat([this.transcriptBuffer, bytes]);
				if (this.transcriptBuffer.length > MAX_TRANSCRIPT_BYTES) {
					this.transcriptBuffer = this.transcriptBuffer.subarray(
						this.transcriptBuffer.length - MAX_TRANSCRIPT_BYTES,
					);
				}
			}
			this.onEvent?.(ptyEvent);
		} catch (error) {
			console.error("invalid AgentOS CDP binding payload", error);
		}
	}

	private async evaluate<T>(expression: string): Promise<T> {
		const result = await this.cdp.send("Runtime.evaluate", {
			expression,
			awaitPromise: true,
			returnByValue: true,
			userGesture: true,
		});
		if (result.exceptionDetails) {
			throw new Error(
				result.exceptionDetails.exception?.description ??
					result.exceptionDetails.text ??
					"CDP Runtime.evaluate failed",
			);
		}
		return result.result.value as T;
	}

	transcript(): string {
		return this.transcriptBuffer.toString("utf8");
	}

	checkpoint(): number {
		return this.transcriptBuffer.length;
	}

	plainTranscript(): string {
		return stripVTControlCharacters(this.transcript());
	}

	async write(data: string | Uint8Array): Promise<void> {
		const base64 = Buffer.from(data).toString("base64");
		await this.evaluate(
			`window.__agentOsBrowserbase.writeBase64(${JSON.stringify(base64)})`,
		);
	}

	async state(): Promise<BrowserVmState> {
		return this.evaluate<BrowserVmState>("window.__agentOsBrowserbase.state()");
	}

	async waitFor(
		needle: string | RegExp,
		timeoutMs = 30_000,
		fromByte = 0,
	): Promise<string> {
		const deadline = Date.now() + timeoutMs;
		while (Date.now() < deadline) {
			const transcript = this.transcriptBuffer
				.subarray(fromByte)
				.toString("utf8");
			const plainTranscript = stripVTControlCharacters(transcript);
			if (
				typeof needle === "string"
					? transcript.includes(needle) || plainTranscript.includes(needle)
					: needle.test(transcript) || needle.test(plainTranscript)
			) {
				return transcript;
			}
			await delay(25);
		}
		throw new Error(
			`timed out waiting for ${String(needle)} after transcript byte ${fromByte}; terminal tail:\n${this.plainTranscript().slice(-4_000)}`,
		);
	}

	async close(): Promise<void> {
		if (this.closed) return;
		this.closed = true;
		await this.evaluate("window.__agentOsBrowserbase.dispose()").catch(
			(error) => {
				console.warn("failed to dispose browser AgentOS runtime", error);
			},
		);
		await this.cdp.detach().catch((error) => {
			console.warn("failed to detach CDP session", error);
		});
		await this.browser.close().catch((error) => {
			console.warn("failed to close Browserbase CDP connection", error);
		});
		await releaseBrowserbaseSession(this.creds, this.sessionId).catch(
			(error) => {
				console.warn("failed to release Browserbase session", error);
			},
		);
		await stopProcess(this.vite, "Vite");
	}
}
