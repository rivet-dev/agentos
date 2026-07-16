import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test } from "vitest";
import { createClient } from "../src/client.js";

const RUN_E2E = process.env.AGENTOS_ACTOR_E2E === "1";
const DEBUG_E2E = process.env.AGENTOS_ACTOR_E2E_DEBUG === "1";
const NAMESPACE = "default";
const TOKEN = "dev";
const POOL_NAME = "agentos-e2e";
const MAX_CAPTURED_LOG_BYTES = 1024 * 1024;
const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const workspaceRoot = resolve(packageRoot, "../..");
const fixturePath = join(
	packageRoot,
	"tests/fixtures/actor-runtime-server.mjs",
);
const sidecarPath = process.env.AGENTOS_SIDECAR_BIN
	? resolve(process.env.AGENTOS_SIDECAR_BIN)
	: join(workspaceRoot, "target/debug/agentos-sidecar");

interface RuntimeHandle {
	child: ChildProcess;
	engine: ChildProcess;
	endpoint: string;
	logs(): string;
	stop(): Promise<void>;
}

const activeRuntimes = new Set<RuntimeHandle>();
const require_ = createRequire(import.meta.url);

function resolveEngineBinary(): string {
	const rivetkitRequire = createRequire(require_.resolve("rivetkit"));
	return (
		rivetkitRequire("@rivetkit/engine-cli") as { getEnginePath(): string }
	).getEnginePath();
}

afterEach(async () => {
	await Promise.all([...activeRuntimes].map((runtime) => runtime.stop()));
});

function appendBounded(current: string, chunk: Buffer): string {
	const combined = current + chunk.toString();
	return combined.length <= MAX_CAPTURED_LOG_BYTES
		? combined
		: combined.slice(combined.length - MAX_CAPTURED_LOG_BYTES);
}

async function stopChildProcess(
	processChild: ChildProcess,
	timeoutMs = 10_000,
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

async function getFreePort(): Promise<number> {
	return await new Promise((resolvePort, reject) => {
		const server = createServer();
		server.unref();
		server.on("error", reject);
		server.listen(0, "127.0.0.1", () => {
			const address = server.address();
			server.close(() => {
				if (!address || typeof address === "string") {
					reject(new Error("failed to allocate actor E2E port"));
					return;
				}
				resolvePort(address.port);
			});
		});
	});
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

async function startRuntime(
	storagePath: string,
	requestedPort?: number,
): Promise<RuntimeHandle> {
	if (!existsSync(sidecarPath)) {
		throw new Error(
			`actor E2E requires ${sidecarPath}; run cargo build -p agentos-sidecar`,
		);
	}
	const port = requestedPort ?? (await getFreePort());
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
			RIVET__API_PEER__PORT: String(port + 1),
			RIVET__METRICS__HOST: "127.0.0.1",
			RIVET__METRICS__PORT: String(port + 10),
			RIVET__FILE_SYSTEM__PATH: dbPath,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	engine.stdout?.on("data", (chunk: Buffer) => {
		stdout = appendBounded(stdout, chunk);
		if (DEBUG_E2E)
			process.stderr.write(`[actor-e2e-engine] ${chunk.toString()}`);
	});
	engine.stderr?.on("data", (chunk: Buffer) => {
		stderr = appendBounded(stderr, chunk);
		if (DEBUG_E2E)
			process.stderr.write(`[actor-e2e-engine] ${chunk.toString()}`);
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
			AGENTOS_E2E_POOL_NAME: POOL_NAME,
			AGENTOS_SIDECAR_BIN: sidecarPath,
			RIVET_NAMESPACE: NAMESPACE,
			RIVET_TOKEN: TOKEN,
			RIVETKIT_ENGINE_SPAWN: "never",
			RIVETKIT_STORAGE_PATH: storagePath,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	child.stdout?.on("data", (chunk: Buffer) => {
		stdout = appendBounded(stdout, chunk);
		if (DEBUG_E2E) process.stderr.write(`[actor-e2e] ${chunk.toString()}`);
	});
	child.stderr?.on("data", (chunk: Buffer) => {
		stderr = appendBounded(stderr, chunk);
		if (DEBUG_E2E) process.stderr.write(`[actor-e2e] ${chunk.toString()}`);
	});
	let stopped = false;
	const runtime: RuntimeHandle = {
		child,
		engine,
		endpoint,
		logs,
		async stop() {
			if (stopped) return;
			stopped = true;
			activeRuntimes.delete(runtime);
			await stopChildProcess(child);
			await stopChildProcess(engine);
		},
	};
	activeRuntimes.add(runtime);
	try {
		const auth = { Authorization: `Bearer ${TOKEN}` };
		const datacentersResponse = await fetch(
			`${endpoint}/datacenters?namespace=${NAMESPACE}`,
			{ headers: auth },
		);
		expect(datacentersResponse.ok, logs()).toBe(true);
		const datacenters = (await datacentersResponse.json()) as {
			datacenters: Array<{ name: string }>;
		};
		const datacenter = datacenters.datacenters[0]?.name;
		if (!datacenter) {
			throw new Error(`engine returned no datacenters\n${logs()}`);
		}
		await waitUntil(
			"runner config registration",
			async () => {
				const response = await fetch(
					`${endpoint}/runner-configs/${POOL_NAME}?namespace=${NAMESPACE}`,
					{
						method: "PUT",
						headers: { ...auth, "Content-Type": "application/json" },
						body: JSON.stringify({
							datacenters: { [datacenter]: { normal: {} } },
						}),
					},
				);
				return response.ok;
			},
			child,
			logs,
		);
		await waitUntil(
			"envoy registration",
			async () => {
				const response = await fetch(
					`${endpoint}/envoys?namespace=${NAMESPACE}&name=${POOL_NAME}`,
					{ headers: auth },
				);
				if (!response.ok) return false;
				const body = (await response.json()) as { envoys: unknown[] };
				return body.envoys.length > 0;
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

function actorHandle(endpoint: string, key: string) {
	const client = createClient<never>({
		endpoint,
		token: TOKEN,
		namespace: NAMESPACE,
		poolName: POOL_NAME,
		disableMetadataLookup: true,
	} as never);
	// The fixture is a separate process, so its inferred registry type is not
	// available here. Runtime assertions cover the complete action contract.
	return (client as any).vm.getOrCreate(key);
}

function bytes(value: unknown): Uint8Array {
	if (value instanceof Uint8Array) return value;
	if (
		Array.isArray(value) &&
		value[0] === "$Uint8Array" &&
		typeof value[1] === "string"
	) {
		return Buffer.from(value[1], "base64");
	}
	throw new TypeError(`expected Uint8Array, received ${String(value)}`);
}

describe.skipIf(!RUN_E2E)("AgentOS real Rivet actor", () => {
	test("persists direct-UDS filesystem chunks across sleep and engine restart", async () => {
		const storagePath = mkdtempSync(join(tmpdir(), "agentos-actor-e2e-"));
		const actorKey = `persistence-${Date.now()}`;
		let runtime: RuntimeHandle | undefined;
		try {
			runtime = await startRuntime(storagePath);
			let handle = actorHandle(runtime.endpoint, actorKey);

			expect(await handle.echo("custom-action-ok")).toBe("custom-action-ok");
			expect(await handle.getWakeCount()).toBe(1);
			await handle.mkdir("/persist");
			await handle.writeFile("/persist/message.txt", "survives sleep");
			const large = new Uint8Array(2 * 1024 * 1024 + 17);
			for (let index = 0; index < large.length; index += 1) {
				large[index] = index % 251;
			}
			await handle.writeFile("/persist/chunked.bin", large);

			const storage = await handle.inspectAgentOsStorage();
			expect(storage.tables).toEqual([
				"agentos_vfs_blocks",
				"agentos_vfs_metadata_chunks",
				"agentos_vfs_metadata_heads",
			]);
			expect(storage.metadataCount).toBe(1);
			expect(storage.metadataChunkCount).toBeGreaterThan(0);
			expect(storage.metadataChunkBytes).toBeGreaterThan(0);
			expect(storage.blockCount).toBeGreaterThan(0);
			expect(storage.blockBytes).toBeGreaterThan(0);

			await handle.sleepActor();
			await new Promise((resolveDelay) => setTimeout(resolveDelay, 1_000));
			expect(await handle.getWakeCount()).toBe(2);
			expect(
				new TextDecoder().decode(
					bytes(await handle.readFile("/persist/message.txt")),
				),
			).toBe("survives sleep");
			expect(bytes(await handle.readFile("/persist/chunked.bin"))).toEqual(
				large,
			);

			const restartPort = Number(new URL(runtime.endpoint).port);
			await runtime.stop();
			runtime = await startRuntime(storagePath, restartPort);
			handle = actorHandle(runtime.endpoint, actorKey);
			expect(
				new TextDecoder().decode(
					bytes(await handle.readFile("/persist/message.txt")),
				),
			).toBe("survives sleep");
			expect(bytes(await handle.readFile("/persist/chunked.bin"))).toEqual(
				large,
			);
			expect(await handle.getWakeCount()).toBe(3);
		} finally {
			await runtime?.stop();
			rmSync(storagePath, { recursive: true, force: true });
		}
	}, 180_000);
});
