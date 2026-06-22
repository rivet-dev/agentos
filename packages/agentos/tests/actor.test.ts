import { type ChildProcess, execFile, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { createServer } from "node:net";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type {
	ActorFactoryHandle,
	CoreRuntime,
	NapiNativePluginOptions,
} from "rivetkit";
import { createClient } from "rivetkit/client";
import {
	agentOs,
	buildConfigJson,
	getPluginPath,
	nodeModulesMount,
} from "../src/index.js";

const testDir = dirname(fileURLToPath(import.meta.url));
function findRepoRoot(start: string): string {
	let current = start;
	while (true) {
		const manifest = join(current, "Cargo.toml");
		if (
			existsSync(manifest) &&
			readFileSync(manifest, "utf8").includes("crates/agentos-actor-plugin")
		) {
			return current;
		}
		const parent = dirname(current);
		if (parent === current) {
			throw new Error(`failed to find agent-os repo root from ${start}`);
		}
		current = parent;
	}
}

const repoRoot = findRepoRoot(testDir);
const r6Root = join(repoRoot, "..", "r6");
const r6RivetkitPackageRoot = join(
	r6Root,
	"rivetkit-typescript",
	"packages",
	"rivetkit",
);
const runtimeFixturePath = join(
	testDir,
	"fixtures",
	"agentos-runtime-server.ts",
);
const tsxLoaderPath = join(
	r6RivetkitPackageRoot,
	"node_modules",
	"tsx",
	"dist",
	"loader.mjs",
);
const execFileAsync = promisify(execFile);
let runtime: ChildProcess | undefined;
let runtimeLogs = { stdout: "", stderr: "" };
const pluginFilename =
	process.platform === "darwin"
		? "libagentos_actor_plugin.dylib"
		: process.platform === "win32"
			? "agentos_actor_plugin.dll"
			: "libagentos_actor_plugin.so";

function bytesToString(value: unknown): string {
	if (value instanceof Uint8Array) return Buffer.from(value).toString("utf8");
	if (Array.isArray(value)) return Buffer.from(value).toString("utf8");
	if (typeof value === "string") return value;
	throw new Error(`unexpected readFile result: ${String(value)}`);
}

function childOutput(): string {
	return [runtimeLogs.stdout, runtimeLogs.stderr].filter(Boolean).join("\n");
}

async function stopRuntime(child: ChildProcess): Promise<void> {
	if (child.exitCode !== null) return;
	child.kill("SIGINT");
	await new Promise<void>((resolve) => {
		const timeout = setTimeout(() => {
			if (child.exitCode === null) child.kill("SIGKILL");
		}, 5_000);
		child.once("exit", () => {
			clearTimeout(timeout);
			resolve();
		});
	});
}

async function getFreePort(): Promise<number> {
	return await new Promise((resolve, reject) => {
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
				resolve(address.port);
			});
		});
	});
}

async function waitForHealth(endpoint: string, timeoutMs: number): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (runtime?.exitCode !== null && runtime !== undefined) {
			throw new Error(
				`agentos runtime exited before health check passed:\n${childOutput()}`,
			);
		}
		try {
			const response = await fetch(`${endpoint}/health`);
			if (response.ok) return;
		} catch {}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw new Error(
		`timed out waiting for engine health at ${endpoint}\n${childOutput()}`,
	);
}

async function upsertNormalRunnerConfig(
	endpoint: string,
	namespace: string,
	token: string | undefined,
	poolName: string,
): Promise<void> {
	const authHeaders = token ? { Authorization: `Bearer ${token}` } : {};
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(namespace)}`,
		{ headers: authHeaders },
	);
	if (!datacentersResponse.ok) {
		throw new Error(
			`failed to list datacenters: ${datacentersResponse.status} ${await datacentersResponse.text()}`,
		);
	}
	const datacentersBody = (await datacentersResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = datacentersBody.datacenters[0]?.name;
	if (!datacenter) throw new Error("engine returned no datacenters");

	const response = await fetch(
		`${endpoint}/runner-configs/${encodeURIComponent(poolName)}?namespace=${encodeURIComponent(namespace)}`,
		{
			method: "PUT",
			headers: {
				...authHeaders,
				"Content-Type": "application/json",
			},
			body: JSON.stringify({
				datacenters: {
					[datacenter]: {
						normal: {},
					},
				},
			}),
		},
	);
	if (!response.ok) {
		throw new Error(
			`failed to upsert runner config ${poolName}: ${response.status} ${await response.text()}`,
		);
	}
}

async function waitForEnvoy(
	endpoint: string,
	namespace: string,
	token: string | undefined,
	poolName: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	const authHeaders = token ? { Authorization: `Bearer ${token}` } : {};
	while (Date.now() < deadline) {
		if (runtime?.exitCode !== null && runtime !== undefined) {
			throw new Error(
				`agentos runtime exited before envoy registration:\n${childOutput()}`,
			);
		}
		const response = await fetch(
			`${endpoint}/envoys?namespace=${encodeURIComponent(namespace)}&name=${encodeURIComponent(poolName)}`,
			{ headers: authHeaders },
		);
		if (response.ok) {
			const body = (await response.json()) as {
				envoys: Array<{ envoy_key: string }>;
			};
			if (body.envoys.length > 0) return;
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw new Error(
		`timed out waiting for envoy registration in ${poolName}\n${childOutput()}`,
	);
}

async function waitForActorReady<T>(
	callback: () => Promise<T>,
	timeoutMs: number,
): Promise<T> {
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;
	while (Date.now() < deadline) {
		try {
			return await callback();
		} catch (error) {
			lastError = error;
			const message = error instanceof Error ? error.message : String(error);
			const code =
				typeof error === "object" &&
				error !== null &&
				"code" in error &&
				typeof error.code === "string"
					? error.code
					: undefined;
			if (
				!(
					(code &&
						/^(no_envoys|actor_ready_timeout|actor_wake_retries_exceeded|service_unavailable)$/.test(
							code,
						)) ||
					/(no_envoys|actor_ready_timeout|actor_wake_retries_exceeded|service_unavailable)/.test(
						message,
					)
				)
			) {
				throw error instanceof Error
					? new Error(`${error.message}\n${childOutput()}`, {
							cause: error,
						})
					: error;
			}
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw lastError instanceof Error
		? lastError
		: new Error("timed out waiting for actor readiness");
}

describe("@rivet-dev/agentos native plugin package bridge", () => {
	beforeAll(async () => {
		await execFileAsync(
			"cargo",
			[
				"build",
				"--manifest-path",
				join(repoRoot, "Cargo.toml"),
				"-p",
				"agentos-sidecar",
				"-p",
				"agentos-actor-plugin",
			],
			{
				cwd: repoRoot,
				env: process.env,
				maxBuffer: 1024 * 1024 * 20,
			},
		);
		const sidecarPath = join(repoRoot, "target", "debug", "agentos-sidecar");
		expect(existsSync(sidecarPath)).toBe(true);
		process.env.AGENTOS_SIDECAR_BIN = sidecarPath;
		process.env.AGENTOS_PLUGIN_BIN = join(
			repoRoot,
			"target",
			"debug",
			pluginFilename,
		);
		expect(existsSync(process.env.AGENTOS_PLUGIN_BIN)).toBe(true);
		const r6EngineBinary = join(r6Root, "target", "debug", "rivet-engine");
		if (existsSync(r6EngineBinary)) {
			process.env.RIVET_ENGINE_BINARY = r6EngineBinary;
		}
	}, 120_000);

	afterEach(async () => {
		if (runtime) {
			await stopRuntime(runtime);
			runtime = undefined;
		}
	}, 30_000);

	test("resolves the dev-built actor plugin cdylib", () => {
		const pluginPath = getPluginPath();
		expect(pluginPath).toBe(
			join(repoRoot, "target", "debug", pluginFilename),
		);
		expect(existsSync(pluginPath)).toBe(true);
	});

	test("serializes config and hands plugin paths to the NAPI runtime", () => {
		const definition = agentOs({
			options: {
				additionalInstructions: ["stay deterministic"],
				loopbackExemptPorts: [4020],
				mounts: [nodeModulesMount("/host/project/node_modules")],
				sidecar: { kind: "shared", pool: "agentos-smoke" },
			},
		});
		const expectedHandle = Symbol("native-factory") as unknown as ActorFactoryHandle;
		const calls: NapiNativePluginOptions[] = [];
		const runtime = {
			kind: "napi",
			createNativePluginFactory(options: NapiNativePluginOptions) {
				calls.push(options);
				return expectedHandle;
			},
		} as CoreRuntime;

		const handle = definition.nativeFactoryBuilder?.(runtime);

		expect(handle).toBe(expectedHandle);
		expect(calls).toHaveLength(1);
		expect(calls[0].pluginPath).toBe(getPluginPath());
		expect(calls[0].sidecarPath).toBe(process.env.AGENTOS_SIDECAR_BIN);
		expect(JSON.parse(calls[0].configJson)).toMatchObject({
			additionalInstructions: ["stay deterministic"],
			loopbackExemptPorts: [4020],
			sidecar: { pool: "agentos-smoke" },
			mounts: [
				{
					path: "/root/node_modules",
					plugin: {
						id: "host_dir",
						config: {
							hostPath: "/host/project/node_modules",
							readOnly: true,
						},
					},
					readOnly: true,
				},
			],
		});
	});

	test("buildConfigJson keeps software descriptors pointed at package roots", () => {
		const configJson = buildConfigJson({
			options: {
				software: [
					{ commandDir: "/abs/wasm-command" },
					{ packageDir: "/abs/agent-package", agent: {} },
					{ packageDir: "/abs/tool-package", hostTool: {} },
				],
			},
			preview: {
				defaultExpiresInSeconds: 3600,
				maxExpiresInSeconds: 86400,
			},
		});

		expect(JSON.parse(configJson).software).toEqual([
			{ package: "/abs/wasm-command" },
			{ package: "/abs/agent-package", kind: "agent" },
			{ package: "/abs/tool-package", kind: "tool" },
		]);
	});

	test("auto-derives /root/node_modules mount from an agent's installed package dir", () => {
		const config = JSON.parse(
			buildConfigJson({
				options: {
					software: [
						{ commandDir: "/proj/node_modules/@agent-os-pkgs/coreutils/wasm" },
						{
							packageDir: "/proj/node_modules/@rivet-dev/agentos-pi",
							requires: [
								"@rivet-dev/agentos-pi",
								"@mariozechner/pi-coding-agent",
							],
							agent: { id: "pi" },
						},
					],
				},
			} as never),
		);

		expect(config.mounts).toEqual([
			{
				path: "/root/node_modules",
				plugin: {
					id: "host_dir",
					config: { hostPath: "/proj/node_modules", readOnly: true },
				},
				readOnly: true,
			},
		]);
	});

	test("explicit /root/node_modules mount overrides the auto-derived one", () => {
		const config = JSON.parse(
			buildConfigJson({
				options: {
					software: [
						{
							packageDir: "/proj/node_modules/@rivet-dev/agentos-pi",
							agent: { id: "pi" },
						},
					],
					mounts: [nodeModulesMount("/custom/node_modules")],
				},
			} as never),
		);

		expect(config.mounts).toHaveLength(1);
		expect(config.mounts[0].plugin.config.hostPath).toBe(
			"/custom/node_modules",
		);
	});

	test("does not auto-mount when an agent package is not inside node_modules", () => {
		const config = JSON.parse(
			buildConfigJson({
				options: {
					software: [{ packageDir: "/abs/agent-package", agent: { id: "x" } }],
				},
			} as never),
		);

		expect(config.mounts).toBeUndefined();
	});

	test("boots a VM through the dylib actor and handles filesystem actions", async () => {
		const poolName = `agentos-package-${crypto.randomUUID()}`;
		const namespace = "default";
		const token = "dev";
		const enginePort = await getFreePort();
		let client: Awaited<ReturnType<typeof createClient<any>>> | undefined;
		try {
			const endpoint = `http://127.0.0.1:${enginePort}`;
			runtimeLogs = { stdout: "", stderr: "" };
			runtime = spawn(process.execPath, ["--import", tsxLoaderPath, runtimeFixturePath], {
				cwd: r6RivetkitPackageRoot,
				env: {
					...process.env,
					RIVET_TOKEN: token,
					RIVET_NAMESPACE: namespace,
					RIVETKIT_TEST_ENDPOINT: endpoint,
					RIVETKIT_TEST_POOL_NAME: poolName,
					AGENTOS_TEST_SIDECAR_POOL: poolName,
					RIVET_RUN_ENGINE_HOST: "127.0.0.1",
					RIVET_RUN_ENGINE_PORT: String(enginePort),
					ESBK_TSCONFIG_PATH: join(
						r6RivetkitPackageRoot,
						"tsconfig.json",
					),
					TSX_TSCONFIG_PATH: join(
						r6RivetkitPackageRoot,
						"tsconfig.json",
					),
					RIVETKIT_STORAGE_PATH: mkdtempSync(
						join(tmpdir(), "agentos-package-smoke-"),
					),
				},
				stdio: ["ignore", "pipe", "pipe"],
			});
			runtime.stdout?.on("data", (chunk) => {
				runtimeLogs.stdout += chunk.toString();
			});
			runtime.stderr?.on("data", (chunk) => {
				runtimeLogs.stderr += chunk.toString();
			});

			await waitForHealth(endpoint, 90_000);
			await upsertNormalRunnerConfig(
				endpoint,
				namespace,
				token,
				poolName,
			);
			await waitForEnvoy(
				endpoint,
				namespace,
				token,
				poolName,
				30_000,
			);
			client = createClient<any>({
				endpoint,
				token,
				namespace,
				poolName,
				disableMetadataLookup: true,
			});
			const handle = await waitForActorReady(
				() =>
					(client as any).os.create([
						`agentos-package-${crypto.randomUUID()}`,
					]),
				30_000,
			);

			await waitForActorReady(
				() =>
					handle.writeFile(
						"/tmp/agentos-package-smoke.txt",
						"hello dylib",
					),
				30_000,
			);

			expect(
				bytesToString(
					await waitForActorReady(
						() => handle.readFile("/tmp/agentos-package-smoke.txt"),
						30_000,
					),
				),
			).toBe("hello dylib");
		} finally {
			await client?.dispose();
			if (runtime) {
				await stopRuntime(runtime);
				runtime = undefined;
			}
		}
	}, 120_000);
});
