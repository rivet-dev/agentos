// Actor-backed shell VM (the `--actor` flag): drives the exact same shell
// surface as the in-process `AgentOs` core client, but through the RivetKit
// agentOS actor (Rivet engine + envoy + AgentOS sidecar).
//
// The launcher starts the engine binary bundled with RivetKit, registers an
// envoy, and exposes the actor through the ordinary client API. Owning both
// child handles ensures `dispose()` cannot leak Rivet's normally persistent
// development engine.

import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createClient } from "@rivet-dev/agentos/client";
import type {
	MountConfig,
	ShellData,
	SoftwareInput,
} from "@rivet-dev/agentos-core";

const __dirname = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = resolve(__dirname, "../../..");
const require_ = createRequire(import.meta.url);

const NAMESPACE = "default";
const TOKEN = "dev";
const POOL_NAME = "agentos-shell";
const MAX_CAPTURED_LOG_BYTES = 1024 * 1024;

function resolveEngineBinary(): string {
	const rivetkitRequire = createRequire(require_.resolve("rivetkit"));
	return (
		rivetkitRequire("@rivetkit/engine-cli") as { getEnginePath(): string }
	).getEnginePath();
}

function appendBounded(current: string, chunk: Buffer): string {
	const combined = current + chunk.toString();
	return combined.length <= MAX_CAPTURED_LOG_BYTES
		? combined
		: combined.slice(combined.length - MAX_CAPTURED_LOG_BYTES);
}

async function stopChildProcess(
	processChild: ChildProcess,
	timeoutMs = 5_000,
): Promise<void> {
	if (processChild.exitCode !== null) return;
	processChild.kill("SIGINT");
	await new Promise<void>((resolveExit) => {
		const timeout = setTimeout(() => {
			if (processChild.exitCode === null) processChild.kill("SIGKILL");
			resolveExit();
		}, timeoutMs);
		if (processChild.exitCode !== null) {
			clearTimeout(timeout);
			resolveExit();
			return;
		}
		processChild.once("exit", () => {
			clearTimeout(timeout);
			resolveExit();
		});
	});
}

export interface ActorShellVmOptions {
	software: SoftwareInput[];
	mounts: MountConfig[];
	defaultSoftware?: boolean;
	limits?: unknown;
}

/**
 * The subset of the `AgentOs` surface the shell CLI drives. The in-process
 * core client satisfies this directly (its `spawn`/`openShell` return
 * synchronously — hence the sync|Promise unions); the actor backend returns
 * promises for everything.
 */
export interface ShellVmHandle {
	spawn(
		command: string,
		args: string[],
		options: {
			cwd?: string;
			env?: Record<string, string>;
			streamStdin?: boolean;
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): { pid: number } | Promise<{ pid: number }>;
	writeProcessStdin(pid: number, data: Uint8Array | string): Promise<void>;
	closeProcessStdin(pid: number): Promise<void>;
	waitProcess(pid: number): Promise<number>;
	openShell(options: {
		command?: string;
		args?: string[];
		cwd?: string;
		env?: Record<string, string>;
		cols?: number;
		rows?: number;
		/** Optional stderr-only diagnostic tap; do not render it with `onShellData`. */
		onStderr?: (data: Uint8Array) => void;
	}): { shellId: string } | Promise<{ shellId: string }>;
	writeShell(shellId: string, data: Uint8Array | string): Promise<void>;
	resizeShell(shellId: string, cols: number, rows: number): void;
	/** Ordered PTY output containing stdout and stderr exactly once. */
	onShellData(shellId: string, handler: (event: ShellData) => void): () => void;
	waitShell(shellId: string): Promise<number>;
	dispose(): Promise<void>;
}

async function getFreePort(): Promise<number> {
	return await new Promise((resolvePort, reject) => {
		const server = createServer();
		server.unref();
		server.on("error", reject);
		server.listen(0, "127.0.0.1", () => {
			const address = server.address();
			server.close(() => {
				if (!address || typeof address === "string") {
					reject(new Error("failed to allocate a TCP port"));
					return;
				}
				resolvePort(address.port);
			});
		});
	});
}

async function waitForHealth(
	endpoint: string,
	child: ChildProcess,
	logs: () => string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;
	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`actor runtime exited before the engine became healthy:\n${logs()}`,
			);
		}
		try {
			const response = await fetch(`${endpoint}/health`);
			if (response.ok) return;
		} catch (error) {
			lastError = error;
		}
		await new Promise((r) => setTimeout(r, 300));
	}
	throw new Error(
		`timed out waiting for engine health at ${endpoint}: ${String(lastError)}\n${logs()}`,
	);
}

async function upsertRunnerConfig(endpoint: string): Promise<void> {
	const auth = { Authorization: `Bearer ${TOKEN}` };
	const dcResponse = await fetch(
		`${endpoint}/datacenters?namespace=${NAMESPACE}`,
		{ headers: auth },
	);
	if (!dcResponse.ok) {
		throw new Error(`failed to list datacenters: ${dcResponse.status}`);
	}
	const body = (await dcResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = body.datacenters[0]?.name;
	if (!datacenter) throw new Error("engine returned no datacenters");
	const response = await fetch(
		`${endpoint}/runner-configs/${POOL_NAME}?namespace=${NAMESPACE}`,
		{
			method: "PUT",
			headers: { ...auth, "Content-Type": "application/json" },
			body: JSON.stringify({ datacenters: { [datacenter]: { normal: {} } } }),
		},
	);
	if (!response.ok) {
		throw new Error(`failed to upsert runner config: ${response.status}`);
	}
}

async function waitForEnvoy(
	endpoint: string,
	child: ChildProcess,
	logs: () => string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	const auth = { Authorization: `Bearer ${TOKEN}` };
	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`actor runtime exited before envoy registration:\n${logs()}`,
			);
		}
		const response = await fetch(
			`${endpoint}/envoys?namespace=${NAMESPACE}&name=${POOL_NAME}`,
			{ headers: auth },
		);
		if (response.ok) {
			const body = (await response.json()) as { envoys: unknown[] };
			if (body.envoys.length > 0) return;
		}
		await new Promise((r) => setTimeout(r, 300));
	}
	throw new Error(`timed out waiting for envoy registration\n${logs()}`);
}

/** Decode an event `data` field: real Uint8Array, `["$Uint8Array", b64]`, or utf8 string. */
function toBytes(data: unknown): Uint8Array {
	if (data instanceof Uint8Array) return data;
	if (
		Array.isArray(data) &&
		data.length === 2 &&
		data[0] === "$Uint8Array" &&
		typeof data[1] === "string"
	) {
		return Buffer.from(data[1], "base64");
	}
	return Buffer.from(String(data ?? ""), "utf8");
}

export async function createActorShellVm(
	options: ActorShellVmOptions,
): Promise<ShellVmHandle> {
	const tsxLoaderPath = require_.resolve("tsx");

	// Dev convenience: prefer the workspace debug sidecar/plugin builds when the
	// platform npm packages are not installed.
	if (!process.env.AGENTOS_SIDECAR_BIN) {
		const debugSidecar = join(
			workspaceRoot,
			"target",
			"debug",
			"agentos-sidecar",
		);
		if (existsSync(debugSidecar)) {
			process.env.AGENTOS_SIDECAR_BIN = debugSidecar;
		}
	}

	const enginePort = await getFreePort();
	const endpoint = `http://127.0.0.1:${enginePort}`;
	const compiledServerPath = join(__dirname, "actor-server.js");
	const serverPath = existsSync(compiledServerPath)
		? compiledServerPath
		: join(__dirname, "actor-server.ts");
	const storagePath = mkdtempSync(join(tmpdir(), "agentos-shell-actor-"));
	const engineDbPath = join(storagePath, "var/engine/db");
	mkdirSync(engineDbPath, { recursive: true });
	const logs = { stdout: "", stderr: "" };
	const debugLogs = process.env.AGENTOS_SHELL_ACTOR_DEBUG === "1";
	const engine = spawn(resolveEngineBinary(), ["start"], {
		cwd: workspaceRoot,
		env: {
			...process.env,
			RIVETKIT_STORAGE_PATH: storagePath,
			RIVET__GUARD__HOST: "127.0.0.1",
			RIVET__GUARD__PORT: String(enginePort),
			RIVET__API_PEER__HOST: "127.0.0.1",
			RIVET__API_PEER__PORT: String(enginePort + 1),
			RIVET__METRICS__HOST: "127.0.0.1",
			RIVET__METRICS__PORT: String(enginePort + 10),
			RIVET__FILE_SYSTEM__PATH: engineDbPath,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	engine.stdout?.on("data", (chunk: Buffer) => {
		logs.stdout = appendBounded(logs.stdout, chunk);
		if (debugLogs) process.stderr.write(`[actor-engine] ${chunk}`);
	});
	engine.stderr?.on("data", (chunk: Buffer) => {
		logs.stderr = appendBounded(logs.stderr, chunk);
		if (debugLogs) process.stderr.write(`[actor-engine] ${chunk}`);
	});
	const childLogs = () => [logs.stdout, logs.stderr].filter(Boolean).join("\n");
	try {
		await waitForHealth(endpoint, engine, childLogs, 60_000);
	} catch (error) {
		await stopChildProcess(engine);
		rmSync(storagePath, { recursive: true, force: true });
		throw error;
	}
	const child = spawn(
		process.execPath,
		["--import", tsxLoaderPath, serverPath],
		{
			cwd: workspaceRoot,
			env: {
				...process.env,
				RIVET_TOKEN: TOKEN,
				RIVET_NAMESPACE: NAMESPACE,
				AGENTOS_SHELL_ENDPOINT: endpoint,
				AGENTOS_SHELL_POOL_NAME: POOL_NAME,
				AGENTOS_SHELL_ACTOR_OPTIONS: JSON.stringify({
					software: options.software,
					mounts: options.mounts,
					defaultSoftware: options.defaultSoftware,
					...(options.limits ? { limits: options.limits } : {}),
				}),
				RIVETKIT_ENGINE_SPAWN: "never",
				RIVETKIT_STORAGE_PATH: storagePath,
			},
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	child.stdout?.on("data", (chunk: Buffer) => {
		logs.stdout = appendBounded(logs.stdout, chunk);
		if (debugLogs) process.stderr.write(`[actor-server] ${chunk}`);
	});
	child.stderr?.on("data", (chunk: Buffer) => {
		logs.stderr = appendBounded(logs.stderr, chunk);
		if (debugLogs) process.stderr.write(`[actor-server] ${chunk}`);
	});

	try {
		await upsertRunnerConfig(endpoint);
		await waitForEnvoy(endpoint, child, childLogs, 30_000);
	} catch (error) {
		await stopChildProcess(child);
		await stopChildProcess(engine);
		rmSync(storagePath, { recursive: true, force: true });
		throw error;
	}

	const client = createClient<never>({
		endpoint,
		token: TOKEN,
		namespace: NAMESPACE,
		poolName: POOL_NAME,
		disableMetadataLookup: true,
	} as never);
	const handle = (client as any).vm.getOrCreate(`shell-${process.pid}`);
	const conn = handle.connect();

	const shellDataHandlers = new Map<string, Set<(event: ShellData) => void>>();
	const shellStderrHandlers = new Map<
		string,
		Set<(data: Uint8Array) => void>
	>();
	const processStdoutHandlers = new Map<number, (data: Uint8Array) => void>();
	const processStderrHandlers = new Map<number, (data: Uint8Array) => void>();
	// Output can arrive between the openShell reply and the caller's
	// onShellData subscription; buffer it (bounded) and flush on subscribe.
	const PENDING_SHELL_DATA_LIMIT = 256;
	const pendingShellData = new Map<string, ShellData[]>();

	conn.on("shellData", (payload: { shellId: string; data: unknown }) => {
		const event = { shellId: payload.shellId, data: toBytes(payload.data) };
		const handlers = shellDataHandlers.get(payload.shellId);
		if (!handlers || handlers.size === 0) {
			let pending = pendingShellData.get(payload.shellId);
			if (!pending) {
				pending = [];
				pendingShellData.set(payload.shellId, pending);
			}
			if (pending.length < PENDING_SHELL_DATA_LIMIT) pending.push(event);
			return;
		}
		for (const handler of handlers) handler(event);
	});
	conn.on("shellStderr", (payload: { shellId: string; data: unknown }) => {
		const handlers = shellStderrHandlers.get(payload.shellId);
		if (!handlers) return;
		const bytes = toBytes(payload.data);
		for (const handler of handlers) handler(bytes);
	});
	conn.on(
		"processOutput",
		(payload: { pid: number; stream: string; data: unknown }) => {
			const handler =
				payload.stream === "stderr"
					? processStderrHandlers.get(payload.pid)
					: processStdoutHandlers.get(payload.pid);
			if (handler) handler(toBytes(payload.data));
		},
	);

	// The first action triggers actor creation + VM bring-up; retry the
	// scheduling races the same way the actor e2e test does.
	async function withReadyRetry<T>(run: () => Promise<T>): Promise<T> {
		const deadline = Date.now() + 60_000;
		let lastError: unknown;
		while (Date.now() < deadline) {
			try {
				return await run();
			} catch (error) {
				lastError = error;
				const code =
					typeof error === "object" && error !== null && "code" in error
						? String((error as { code: unknown }).code)
						: "";
				if (
					!/^(no_envoys|actor_ready_timeout|actor_wake_retries_exceeded|service_unavailable)$/.test(
						code,
					)
				) {
					throw error;
				}
				await new Promise((r) => setTimeout(r, 1000));
			}
		}
		throw lastError;
	}

	return {
		async spawn(command, args, spawnOptions) {
			const result = (await withReadyRetry(() =>
				handle.spawn(command, args, {
					env: spawnOptions.env,
					cwd: spawnOptions.cwd,
					streamStdin: spawnOptions.streamStdin,
				}),
			)) as { pid: number };
			if (spawnOptions.onStdout) {
				processStdoutHandlers.set(result.pid, spawnOptions.onStdout);
			}
			if (spawnOptions.onStderr) {
				processStderrHandlers.set(result.pid, spawnOptions.onStderr);
			}
			return result;
		},
		async writeProcessStdin(pid, data) {
			await handle.writeProcessStdin(pid, data);
		},
		async closeProcessStdin(pid) {
			await handle.closeProcessStdin(pid);
		},
		async waitProcess(pid) {
			return (await handle.waitProcess(pid)) as number;
		},
		async openShell(shellOptions) {
			const { onStderr, ...rest } = shellOptions;
			const result = (await withReadyRetry(() => handle.openShell(rest))) as {
				shellId: string;
			};
			if (onStderr) {
				let handlers = shellStderrHandlers.get(result.shellId);
				if (!handlers) {
					handlers = new Set();
					shellStderrHandlers.set(result.shellId, handlers);
				}
				handlers.add(onStderr);
			}
			return result;
		},
		async writeShell(shellId, data) {
			await handle.writeShell(shellId, data);
		},
		resizeShell(shellId, cols, rows) {
			void handle.resizeShell(shellId, cols, rows).catch((error: unknown) => {
				process.stderr.write(`resizeShell failed: ${String(error)}\n`);
			});
		},
		onShellData(shellId, handler) {
			let handlers = shellDataHandlers.get(shellId);
			if (!handlers) {
				handlers = new Set();
				shellDataHandlers.set(shellId, handlers);
			}
			handlers.add(handler);
			const pending = pendingShellData.get(shellId);
			if (pending) {
				pendingShellData.delete(shellId);
				for (const chunk of pending) handler(chunk);
			}
			return () => {
				handlers?.delete(handler);
			};
		},
		async waitShell(shellId) {
			return (await handle.waitShell(shellId)) as number;
		},
		async dispose() {
			try {
				conn.dispose?.();
			} catch (error) {
				process.stderr.write(
					`actor connection dispose failed: ${String(error)}\n`,
				);
			}
			await stopChildProcess(child);
			await stopChildProcess(engine);
			rmSync(storagePath, { recursive: true, force: true });
		},
	};
}
