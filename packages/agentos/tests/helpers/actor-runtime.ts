import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { agentOS, Registry } from "../../src/index.js";
import { createClient } from "../../src/client.js";

const DEBUG_E2E = process.env.AGENTOS_ACTOR_E2E_DEBUG === "1";
export const ACTOR_E2E_NAMESPACE = "default";
export const ACTOR_E2E_TOKEN = "dev";
export const ACTOR_E2E_POOL_NAME = "agentos-e2e";
export const ACTOR_E2E_CONN_PARAMS = { authToken: "e2e-allowed" };
const MAX_CAPTURED_LOG_BYTES = 1024 * 1024;
const packageRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const workspaceRoot = resolve(packageRoot, "../..");
const fixturePath = join(
	packageRoot,
	"tests/fixtures/actor-runtime-server.mjs",
);
const sidecarPath = process.env.AGENTOS_SIDECAR_BIN
	? resolve(process.env.AGENTOS_SIDECAR_BIN)
	: join(workspaceRoot, "target/debug/agentos-sidecar");
const require_ = createRequire(import.meta.url);

export interface ActorRuntimeHandle {
	child: ChildProcess;
	engine: ChildProcess;
	endpoint: string;
	logs(): string;
	stop(): Promise<void>;
}

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
	child: ChildProcess,
	timeoutMs = 10_000,
): Promise<void> {
	if (child.exitCode !== null) return;
	child.kill("SIGINT");
	await new Promise<void>((resolveExit) => {
		const timeout = setTimeout(() => {
			if (child.exitCode === null) child.kill("SIGKILL");
			resolveExit();
		}, timeoutMs);
		if (child.exitCode !== null) {
			clearTimeout(timeout);
			resolveExit();
			return;
		}
		child.once("exit", () => {
			clearTimeout(timeout);
			resolveExit();
		});
	});
}

async function getFreePorts(count: number): Promise<number[]> {
	const servers = Array.from({ length: count }, () => createServer());
	try {
		await Promise.all(
			servers.map(
				(server) =>
					new Promise<void>((resolveListen, reject) => {
						server.unref();
						server.once("error", reject);
						server.listen(0, "127.0.0.1", resolveListen);
					}),
			),
		);
		return servers.map((server) => {
			const address = server.address();
			if (!address || typeof address === "string") {
				throw new Error("failed to allocate actor E2E port");
			}
			return address.port;
		});
	} finally {
		await Promise.all(
			servers.map(
				(server) =>
					new Promise<void>((resolveClose) => {
						if (!server.listening) {
							resolveClose();
							return;
						}
						server.close(() => resolveClose());
					}),
			),
		);
	}
}

async function waitUntil(
	description: string,
	run: () => Promise<boolean>,
	child: ChildProcess,
	logs: () => string,
	timeoutMs = 60_000,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(`${description}: runtime exited\n${logs()}`);
		}
		try {
			if (await run()) return;
		} catch {
			// The engine and envoy endpoints become available independently.
		}
		await new Promise((resolveDelay) => setTimeout(resolveDelay, 200));
	}
	throw new Error(`${description}: timed out\n${logs()}`);
}

export async function startActorRuntime(
	storagePath: string,
	requestedPort?: number,
): Promise<ActorRuntimeHandle> {
	if (!existsSync(sidecarPath)) {
		throw new Error(
			`actor E2E requires ${sidecarPath}; run cargo build -p agentos-sidecar`,
		);
	}
	const allocatedPorts = await getFreePorts(
		requestedPort === undefined ? 3 : 2,
	);
	const port = requestedPort ?? allocatedPorts[0];
	const peerPort = allocatedPorts[requestedPort === undefined ? 1 : 0];
	const metricsPort = allocatedPorts[requestedPort === undefined ? 2 : 1];
	if (
		port === undefined ||
		peerPort === undefined ||
		metricsPort === undefined
	) {
		throw new Error("failed to allocate actor E2E ports");
	}
	const endpoint = `http://127.0.0.1:${port}`;
	let stdout = "";
	let stderr = "";
	const dbPath = join(storagePath, "var/engine/db");
	mkdirSync(dbPath, { recursive: true });
	const engine = spawn(resolveEngineBinary(), ["start"], {
		cwd: workspaceRoot,
		env: {
			...process.env,
			RIVETKIT_STORAGE_PATH: storagePath,
			RIVET__GUARD__HOST: "127.0.0.1",
			RIVET__GUARD__PORT: String(port),
			RIVET__API_PEER__HOST: "127.0.0.1",
			RIVET__API_PEER__PORT: String(peerPort),
			RIVET__METRICS__HOST: "127.0.0.1",
			RIVET__METRICS__PORT: String(metricsPort),
			RIVET__FILE_SYSTEM__PATH: dbPath,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	engine.stdout?.on("data", (chunk: Buffer) => {
		stdout = appendBounded(stdout, chunk);
		if (DEBUG_E2E) process.stderr.write(`[actor-e2e-engine] ${chunk}`);
	});
	engine.stderr?.on("data", (chunk: Buffer) => {
		stderr = appendBounded(stderr, chunk);
		if (DEBUG_E2E) process.stderr.write(`[actor-e2e-engine] ${chunk}`);
	});
	const logs = () => [stdout, stderr].filter(Boolean).join("\n");
	try {
		await waitUntil(
			"engine health",
			async () => (await fetch(`${endpoint}/health`)).ok,
			engine,
			logs,
		);
	} catch (error) {
		await stopChildProcess(engine);
		throw error;
	}

	const child = spawn(process.execPath, [fixturePath], {
		cwd: workspaceRoot,
		env: {
			...process.env,
			AGENTOS_E2E_ENDPOINT: endpoint,
			AGENTOS_E2E_POOL_NAME: ACTOR_E2E_POOL_NAME,
			AGENTOS_SIDECAR_BIN: sidecarPath,
			RIVET_NAMESPACE: ACTOR_E2E_NAMESPACE,
			RIVET_TOKEN: ACTOR_E2E_TOKEN,
			RIVETKIT_ENGINE_SPAWN: "never",
			RIVETKIT_STORAGE_PATH: storagePath,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	child.stdout?.on("data", (chunk: Buffer) => {
		stdout = appendBounded(stdout, chunk);
		if (DEBUG_E2E) process.stderr.write(`[actor-e2e] ${chunk}`);
	});
	child.stderr?.on("data", (chunk: Buffer) => {
		stderr = appendBounded(stderr, chunk);
		if (DEBUG_E2E) process.stderr.write(`[actor-e2e] ${chunk}`);
	});
	let stopped = false;
	const runtime: ActorRuntimeHandle = {
		child,
		engine,
		endpoint,
		logs,
		async stop() {
			if (stopped) return;
			stopped = true;
			await stopChildProcess(child);
			await stopChildProcess(engine);
		},
	};

	try {
		const auth = { Authorization: `Bearer ${ACTOR_E2E_TOKEN}` };
		const datacentersResponse = await fetch(
			`${endpoint}/datacenters?namespace=${ACTOR_E2E_NAMESPACE}`,
			{ headers: auth },
		);
		if (!datacentersResponse.ok) {
			throw new Error(`failed to list datacenters\n${logs()}`);
		}
		const datacenters = (await datacentersResponse.json()) as {
			datacenters: Array<{ name: string }>;
		};
		const datacenter = datacenters.datacenters[0]?.name;
		if (!datacenter)
			throw new Error(`engine returned no datacenters\n${logs()}`);
		await waitUntil(
			"runner config registration",
			async () =>
				(
					await fetch(
						`${endpoint}/runner-configs/${ACTOR_E2E_POOL_NAME}?namespace=${ACTOR_E2E_NAMESPACE}`,
						{
							method: "PUT",
							headers: { ...auth, "Content-Type": "application/json" },
							body: JSON.stringify({
								datacenters: { [datacenter]: { normal: {} } },
							}),
						},
					)
				).ok,
			child,
			logs,
		);
		await waitUntil(
			"envoy registration",
			async () => {
				const response = await fetch(
					`${endpoint}/envoys?namespace=${ACTOR_E2E_NAMESPACE}&name=${ACTOR_E2E_POOL_NAME}`,
					{ headers: auth },
				);
				if (!response.ok) return false;
				return (
					((await response.json()) as { envoys: unknown[] }).envoys.length > 0
				);
			},
			child,
			logs,
		);
		return runtime;
	} catch (error) {
		await runtime.stop();
		throw error;
	}
}

type ActorE2ERegistry = Registry<{ vm: ReturnType<typeof agentOS> }>;

function client(endpoint: string) {
	return createClient<ActorE2ERegistry>({
		endpoint,
		token: ACTOR_E2E_TOKEN,
		namespace: ACTOR_E2E_NAMESPACE,
		poolName: ACTOR_E2E_POOL_NAME,
	});
}

export function actorHandle(
	endpoint: string,
	key: string,
	params: unknown = ACTOR_E2E_CONN_PARAMS,
): any {
	return client(endpoint).vm.getOrCreate(key, { params });
}

export async function createActorHandle(
	endpoint: string,
	key: string,
	input: unknown,
): Promise<any> {
	return client(endpoint).vm.create(key, {
		input,
		params: ACTOR_E2E_CONN_PARAMS,
	});
}

export function actorBytes(value: unknown): Uint8Array {
	if (value instanceof Uint8Array) return value;
	throw new TypeError(`expected Uint8Array, received ${String(value)}`);
}
