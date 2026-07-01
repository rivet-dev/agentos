// Actor-backed shell VM (the `--actor` flag): drives the exact same shell
// surface as the in-process `AgentOs` core client, but through the RivetKit
// agentOS actor (rivet engine + envoy + dylib actor plugin + sidecar).
//
// Bootstrap mirrors `packages/agentos/tests/actor.test.ts`: spawn a runtime
// server child (engine auto-spawned by the native registry), upsert a "normal"
// runner config for the envoy pool, wait for envoy registration, then talk to
// the actor through `createClient`. Requires the `r6` sibling checkout (the
// native registry builder is imported from its rivetkit-typescript source),
// exactly like the actor e2e test.

import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createClient } from "@rivet-dev/agentos/client";
import type { MountConfig, SoftwareInput } from "@rivet-dev/agentos-core";

const __dirname = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = resolve(__dirname, "../../..");
const require_ = createRequire(import.meta.url);

const NAMESPACE = "default";
const TOKEN = "dev";
const POOL_NAME = "agentos-shell";

export interface ActorShellVmOptions {
	software: SoftwareInput[];
	mounts: MountConfig[];
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
		onStderr?: (data: Uint8Array) => void;
	}): { shellId: string } | Promise<{ shellId: string }>;
	writeShell(shellId: string, data: Uint8Array | string): Promise<void>;
	resizeShell(shellId: string, cols: number, rows: number): void;
	onShellData(shellId: string, handler: (data: Uint8Array) => void): () => void;
	waitShell(shellId: string): Promise<number>;
	dispose(): Promise<void>;
}

function r6Root(): string {
	return process.env.AGENTOS_R6_ROOT ?? resolve(workspaceRoot, "..", "r6");
}

function resolveEngineBinary(): string | undefined {
	if (process.env.RIVET_ENGINE_BINARY) return process.env.RIVET_ENGINE_BINARY;
	const r6Engine = join(r6Root(), "target", "debug", "rivet-engine");
	if (existsSync(r6Engine)) return r6Engine;
	try {
		const pkgJson = require_.resolve(
			"@rivetkit/engine-cli-linux-x64-musl/package.json",
		);
		const candidate = join(dirname(pkgJson), "rivet-engine");
		if (existsSync(candidate)) return candidate;
	} catch {
		// platform package not installed; serve() reports binary_unavailable.
	}
	return undefined;
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
	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`actor runtime exited before the engine became healthy:\n${logs()}`,
			);
		}
		try {
			const response = await fetch(`${endpoint}/health`);
			if (response.ok) return;
		} catch {}
		await new Promise((r) => setTimeout(r, 300));
	}
	throw new Error(
		`timed out waiting for engine health at ${endpoint}\n${logs()}`,
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
	const r6 = r6Root();
	const r6RivetkitPackageRoot = join(
		r6,
		"rivetkit-typescript",
		"packages",
		"rivetkit",
	);
	const tsxLoaderPath = join(
		r6RivetkitPackageRoot,
		"node_modules",
		"tsx",
		"dist",
		"loader.mjs",
	);
	if (!existsSync(tsxLoaderPath)) {
		throw new Error(
			`--actor requires the r6 sibling checkout with rivetkit-typescript deps installed (missing ${tsxLoaderPath}); set AGENTOS_R6_ROOT to override`,
		);
	}

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
	const engineBinary = resolveEngineBinary();
	const serverPath = join(__dirname, "actor-server.ts");
	const logs = { stdout: "", stderr: "" };
	const child = spawn(
		process.execPath,
		["--import", tsxLoaderPath, serverPath],
		{
			cwd: r6RivetkitPackageRoot,
			env: {
				...process.env,
				RIVET_TOKEN: TOKEN,
				RIVET_NAMESPACE: NAMESPACE,
				AGENTOS_SHELL_ENDPOINT: endpoint,
				AGENTOS_SHELL_POOL_NAME: POOL_NAME,
				AGENTOS_SHELL_ACTOR_OPTIONS: JSON.stringify({
					software: options.software,
					mounts: options.mounts,
					...(options.limits ? { limits: options.limits } : {}),
				}),
				AGENTOS_R6_ROOT: r6,
				...(engineBinary ? { RIVET_ENGINE_BINARY: engineBinary } : {}),
				RIVET_RUN_ENGINE_HOST: "127.0.0.1",
				RIVET_RUN_ENGINE_PORT: String(enginePort),
				ESBK_TSCONFIG_PATH: join(r6RivetkitPackageRoot, "tsconfig.json"),
				TSX_TSCONFIG_PATH: join(r6RivetkitPackageRoot, "tsconfig.json"),
				RIVETKIT_STORAGE_PATH: mkdtempSync(
					join(tmpdir(), "agentos-shell-actor-"),
				),
			},
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	const debugLogs = process.env.AGENTOS_SHELL_ACTOR_DEBUG === "1";
	child.stdout?.on("data", (chunk) => {
		logs.stdout += chunk.toString();
		if (debugLogs) process.stderr.write(`[actor-server] ${chunk}`);
	});
	child.stderr?.on("data", (chunk) => {
		logs.stderr += chunk.toString();
		if (debugLogs) process.stderr.write(`[actor-server] ${chunk}`);
	});
	const childLogs = () => [logs.stdout, logs.stderr].filter(Boolean).join("\n");

	await waitForHealth(endpoint, child, childLogs, 60_000);
	await upsertRunnerConfig(endpoint);
	await waitForEnvoy(endpoint, child, childLogs, 30_000);

	const client = createClient<never>({
		endpoint,
		token: TOKEN,
		namespace: NAMESPACE,
		poolName: POOL_NAME,
		disableMetadataLookup: true,
	} as never);
	// biome-ignore lint/suspicious/noExplicitAny: untyped registry handle; the action surface mirrors AgentOsActions.
	const handle = (client as any).vm.getOrCreate(`shell-${process.pid}`);
	const conn = handle.connect();

	const shellDataHandlers = new Map<string, Set<(data: Uint8Array) => void>>();
	const shellStderrHandlers = new Map<
		string,
		Set<(data: Uint8Array) => void>
	>();
	const processStdoutHandlers = new Map<number, (data: Uint8Array) => void>();
	const processStderrHandlers = new Map<number, (data: Uint8Array) => void>();
	// Output can arrive between the openShell reply and the caller's
	// onShellData subscription; buffer it (bounded) and flush on subscribe.
	const PENDING_SHELL_DATA_LIMIT = 256;
	const pendingShellData = new Map<string, Uint8Array[]>();

	conn.on("shellData", (payload: { shellId: string; data: unknown }) => {
		const bytes = toBytes(payload.data);
		const handlers = shellDataHandlers.get(payload.shellId);
		if (!handlers || handlers.size === 0) {
			let pending = pendingShellData.get(payload.shellId);
			if (!pending) {
				pending = [];
				pendingShellData.set(payload.shellId, pending);
			}
			if (pending.length < PENDING_SHELL_DATA_LIMIT) pending.push(bytes);
			return;
		}
		for (const handler of handlers) handler(bytes);
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
			} catch {}
			if (child.exitCode === null) {
				child.kill("SIGINT");
				await new Promise<void>((resolveExit) => {
					const timeout = setTimeout(() => {
						if (child.exitCode === null) child.kill("SIGKILL");
						resolveExit();
					}, 5_000);
					child.once("exit", () => {
						clearTimeout(timeout);
						resolveExit();
					});
				});
			}
		},
	};
}
