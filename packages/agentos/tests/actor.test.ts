import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import common from "@agentos-software/common";
import type {
	ActorFactoryHandle,
	CoreRuntime,
	NapiNativePluginOptions,
} from "rivetkit";
import type { createClient } from "rivetkit/client";
import { setupTest } from "rivetkit/test";
import { describe, expect, type TestContext, test } from "vitest";
import {
	agentOS,
	buildConfigJson,
	nodeModulesMount,
	setup,
} from "../src/index.js";
import { INSPECTOR_ACTOR_NAME } from "../src/inspector-tabs/lib/registry.js";

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

function bytesToString(value: unknown): string {
	if (value instanceof Uint8Array) return Buffer.from(value).toString("utf8");
	if (Array.isArray(value)) return Buffer.from(value).toString("utf8");
	if (typeof value === "string") return value;
	throw new Error(`unexpected readFile result: ${String(value)}`);
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
				throw error;
			}
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw lastError instanceof Error
		? lastError
		: new Error("timed out waiting for actor readiness");
}

async function waitForPromise<T>(
	promise: Promise<T>,
	timeoutMs: number,
	label: string,
): Promise<T> {
	let timeout: NodeJS.Timeout | undefined;
	try {
		return await Promise.race([
			promise,
			new Promise<never>((_, reject) => {
				timeout = setTimeout(
					() => reject(new Error(`timed out waiting for ${label}`)),
					timeoutMs,
				);
			}),
		]);
	} finally {
		if (timeout) clearTimeout(timeout);
	}
}

async function configureLocalRunner(
	endpoint: string,
	namespace: string,
	token: string,
	poolName: string,
): Promise<void> {
	const headers = { Authorization: `Bearer ${token}` };
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(namespace)}`,
		{ headers },
	);
	if (!datacentersResponse.ok) {
		throw new Error(
			`failed to list datacenters: ${datacentersResponse.status} ${await datacentersResponse.text()}`,
		);
	}
	const datacenters = (await datacentersResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = datacenters.datacenters[0]?.name;
	if (!datacenter) throw new Error("engine returned no datacenters");

	const response = await fetch(
		`${endpoint}/runner-configs/${encodeURIComponent(poolName)}?namespace=${encodeURIComponent(namespace)}`,
		{
			method: "PUT",
			headers: { ...headers, "Content-Type": "application/json" },
			body: JSON.stringify({
				datacenters: { [datacenter]: { normal: {} } },
			}),
		},
	);
	if (!response.ok) {
		throw new Error(
			`failed to configure runner ${poolName}: ${response.status} ${await response.text()}`,
		);
	}
}

async function waitForLocalEnvoy(
	endpoint: string,
	namespace: string,
	token: string,
	poolName: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	const headers = { Authorization: `Bearer ${token}` };
	while (Date.now() < deadline) {
		const response = await fetch(
			`${endpoint}/envoys?namespace=${encodeURIComponent(namespace)}&name=${encodeURIComponent(poolName)}`,
			{ headers },
		).catch(() => undefined);
		if (response?.ok) {
			const body = (await response.json()) as {
				envoys: Array<unknown>;
			};
			if (body.envoys.length > 0) return;
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	throw new Error(`timed out waiting for local envoy ${poolName}`);
}

describe.sequential("@rivet-dev/agentos actor plugin package bridge", () => {
	test("serializes config and hands plugin options to RivetKit", () => {
		const definition = agentOS({
			additionalInstructions: "stay deterministic",
			loopbackExemptPorts: [4020],
			mounts: [nodeModulesMount("/host/project/node_modules")],
			sidecar: { kind: "shared", pool: "agentos-smoke" },
			actorOptions: {
				sleepTimeout: 500,
				sleepGracePeriod: 1_000,
			},
		});
		const expectedHandle = Symbol(
			"native-factory",
		) as unknown as ActorFactoryHandle;
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
		expect(calls[0].pluginPath).toEqual(expect.any(String));
		expect(calls[0].sidecarPath).toEqual(expect.any(String));
		expect(JSON.parse(calls[0].configJson)).toMatchObject({
			additionalInstructions: "stay deterministic",
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
		expect((calls[0] as any).actorOptions).toMatchObject({
			actionTimeout: 3_600_000,
			sleepTimeout: 500,
			sleepGracePeriod: 1_000,
		});
	});

	test("agentOS flat config keeps callbacks outside native VM options", () => {
		const definition = agentOS({
			defaultSoftware: false,
			software: [],
			onSessionEvent: () => {},
		});
		const expectedHandle = Symbol(
			"native-factory",
		) as unknown as ActorFactoryHandle;
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
		expect(JSON.parse(calls[0].configJson)).toEqual({
			packages: [],
		});
		expect(calls[0].configJson).not.toContain("onSessionEvent");
	});

	test("rejects actor options that cannot cross the NAPI config boundary", () => {
		expect(() =>
			agentOS({
				toolKits: [],
			} as never),
		).toThrow(/toolKits/);

		expect(() =>
			agentOS({
				mounts: [{ path: "/data", driver: {} }],
			} as never),
		).toThrow(/driver/);

		expect(() =>
			agentOS({
				mounts: [
					{
						path: "/data",
						driver: {
							readFile: async () => new Uint8Array(),
						},
					},
				],
			} as never),
		).toThrow(/driver/);

		expect(() =>
			agentOS({
				sidecar: { kind: "explicit", handle: {} },
			} as never),
		).toThrow(/sidecar/);
	});

	test("serializes memory mounts across the package boundary", () => {
		const config = JSON.parse(
			buildConfigJson({
				options: {
					defaultSoftware: false,
					software: [],
					mounts: [
						{
							path: "/data",
							plugin: { id: "memory", config: {} },
						},
					],
				},
			} as never),
		);

		expect(config.mounts).toEqual([
			{
				path: "/data",
				plugin: { id: "memory", config: {} },
			},
		]);
	});

	test("buildConfigJson rejects unknown options instead of dropping them", () => {
		expect(() =>
			buildConfigJson({
				options: {
					notARealOption: true,
				},
			} as never),
		).toThrow(/notARealOption/);
	});

	test("agentOS flat config forwards only VM options to plugin config", () => {
		const definition = agentOS({
			// Disable the default bundle so the software assertion stays deterministic.
			defaultSoftware: false,
			software: [],
			additionalInstructions: "flat public config",
			loopbackExemptPorts: [3000],
			preview: {
				defaultExpiresInSeconds: 60,
				maxExpiresInSeconds: 120,
			},
		});
		const calls: NapiNativePluginOptions[] = [];
		const runtime = {
			kind: "napi",
			createNativePluginFactory(options: NapiNativePluginOptions) {
				calls.push(options);
				return Symbol("native-factory") as unknown as ActorFactoryHandle;
			},
		} as CoreRuntime;

		definition.nativeFactoryBuilder?.(runtime);

		expect(JSON.parse(calls[0].configJson)).toMatchObject({
			packages: [],
			additionalInstructions: "flat public config",
			loopbackExemptPorts: [3000],
		});
		expect(JSON.parse(calls[0].configJson)).not.toHaveProperty("preview");
	});

	test("agentOS actorOptions override actor sleep defaults", () => {
		const definition = agentOS({
			defaultSoftware: false,
			software: [],
			actorOptions: {
				sleepTimeout: 500,
				sleepGracePeriod: 1_000,
			},
		});

		expect((definition as any).config.options).toMatchObject({
			sleepTimeout: 500,
			sleepGracePeriod: 1_000,
		});
	});

	test("inspector action calls target declared actor actions", () => {
		const actorActions = readFileSync(
			join(
				repoRoot,
				"packages",
				"agentos",
				"src",
				"generated",
				"actor-actions.generated.ts",
			),
			"utf8",
		);
		const inspectorSource = readFileSync(
			join(
				repoRoot,
				"packages",
				"agentos",
				"src",
				"inspector-tabs",
				"lib",
				"source.ts",
			),
			"utf8",
		);
		const declared = new Set(
			[...actorActions.matchAll(/^\s*([a-zA-Z][\w]*):\s*\(/gm)].map(
				(match) => match[1],
			),
		);
		const calls = [
			...inspectorSource.matchAll(
				/callAction(?:<[^>]+>)?\("([^"]+)",\s*\[([^\]]*)\]/g,
			),
		].map((match) => ({
			name: match[1],
			args: match[2].trim(),
		}));

		expect(calls.map((call) => call.name).sort()).toEqual([
			"getSessionEvents",
			"listMounts",
			"listPersistedSessions",
			"listProcesses",
			"listSoftware",
			"readFile",
			"readdirEntries",
			"stat",
		]);
		for (const call of calls) {
			expect(declared.has(call.name), `${call.name} is declared`).toBe(true);
		}
		expect(calls.find((call) => call.name === "readdirEntries")?.args).toBe(
			"path",
		);
		expect(calls.find((call) => call.name === "getSessionEvents")?.args).toBe(
			"sessionId",
		);
	});

	test("buildConfigJson keeps software descriptors pointed at package roots", () => {
		const configJson = buildConfigJson({
			options: {
				// Disable the default bundle so this stays focused on the mapping.
				defaultSoftware: false,
				software: [
					"/abs/wasm-command.aospkg",
					{ packagePath: "/abs/project/node_modules/@agentos-software/pi" },
					{ packagePath: "/abs/tool-package.aospkg" },
				],
			},
			preview: {
				defaultExpiresInSeconds: 3600,
				maxExpiresInSeconds: 86400,
			},
		} as never);

		expect(JSON.parse(configJson).packages).toEqual([
			{ packagePath: "/abs/wasm-command.aospkg" },
			{ packagePath: "/abs/project/node_modules/@agentos-software/pi" },
			{ packagePath: "/abs/tool-package.aospkg" },
		]);
	});

	test("auto-injects the default common software bundle unless disabled", () => {
		const withDefault = JSON.parse(
			buildConfigJson({
				options: { software: ["/x/wasm.aospkg"] },
			} as never),
		);
		const pkgs = withDefault.packages.map(
			(s: { packagePath: string }) => s.packagePath,
		);
		expect(pkgs).toContain("/x/wasm.aospkg");
		// common (sh + coreutils + tools) is injected from the software registry.
		expect(pkgs.some((p: string) => p.includes("coreutils"))).toBe(true);
		expect(withDefault.packages.length).toBeGreaterThan(1);

		const noDefault = JSON.parse(
			buildConfigJson({
				options: {
					software: ["/x/wasm.aospkg"],
					defaultSoftware: false,
				},
			} as never),
		);
		expect(noDefault.packages).toEqual([{ packagePath: "/x/wasm.aospkg" }]);
	});

	test("does not duplicate an explicitly-provided default package", () => {
		const onlyDefault = JSON.parse(buildConfigJson({ options: {} } as never))
			.packages.length;
		const withExplicitCommon = JSON.parse(
			buildConfigJson({ options: { software: [common] } } as never),
		).packages.length;
		// Passing common explicitly must not double the injected bundle.
		expect(withExplicitCommon).toBe(onlyDefault);
	});

	test("explicit /root/node_modules mount serializes with package refs", () => {
		const config = JSON.parse(
			buildConfigJson({
				options: {
					software: [
						{
							packagePath: "/proj/node_modules/@agentos-software/pi",
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

	test("rejects removed software descriptor fields instead of dropping them", () => {
		for (const legacy of ["packageTar", "packageDir", "commandDir", "dir"]) {
			expect(() =>
				buildConfigJson({
					options: {
						software: [{ [legacy]: "/abs/old-package" }],
					},
				} as never),
			).toThrow(/software/);
		}
	});

	test("rejects an unrecognized software entry instead of omitting it", () => {
		expect(() =>
			buildConfigJson({
				options: {
					defaultSoftware: false,
					software: [{ packagePath: 42 }],
				},
			} as never),
		).toThrow(/software/);
	});

	async function setupActorTest(c: TestContext): Promise<{
		client: Awaited<ReturnType<typeof createClient<any>>>;
	}> {
		const enginePort = await getFreePort();
		const endpoint = `http://127.0.0.1:${enginePort}`;
		const namespace = "default";
		const token = "dev";
		const poolName = "default";
		const previousStoragePath = process.env.RIVETKIT_STORAGE_PATH;
		process.env.RIVETKIT_STORAGE_PATH = mkdtempSync(
			join(tmpdir(), "agentos-package-smoke-"),
		);
		const registry = setup({
			use: {
				os: agentOS({
					defaultSoftware: false,
					software: [],
					permissions: {
						fs: "allow",
						network: "allow",
						childProcess: "allow",
						process: "allow",
						env: "allow",
					},
					sidecar: { kind: "shared", pool: poolName },
					mounts: [{ path: "/scratch", plugin: { id: "memory", config: {} } }],
				}),
			},
			runtime: "native",
			startEngine: true,
			engineHost: "127.0.0.1",
			enginePort,
			namespace,
			token,
			envoy: { poolName },
			shutdown: { disableSignalHandlers: true },
		});
		c.onTestFinished(async () => {
			try {
				await registry.shutdown();
			} finally {
				if (previousStoragePath === undefined) {
					delete process.env.RIVETKIT_STORAGE_PATH;
				} else {
					process.env.RIVETKIT_STORAGE_PATH = previousStoragePath;
				}
			}
		});
		const result = await setupTest(c, registry);
		await configureLocalRunner(endpoint, namespace, token, poolName);
		await waitForLocalEnvoy(endpoint, namespace, token, poolName, 30_000);
		return result;
	}

	test("runs basic actor operations", async (c) => {
		const { client } = await setupActorTest(c);
		const handle = await waitForActorReady(
			() =>
				(client as any).os.create([`agentos-package-${crypto.randomUUID()}`]),
			30_000,
		);

		await waitForActorReady(
			() => handle.writeFile("/tmp/agentos-package-smoke.txt", "hello actor"),
			30_000,
		);
		const content = await waitForActorReady(
			() => handle.readFile("/tmp/agentos-package-smoke.txt"),
			30_000,
		);
		expect(bytesToString(content)).toBe("hello actor");

		const gatewayPath = `/tmp/gateway-${crypto.randomUUID()}.txt`;
		await waitForPromise(
			handle.action({
				name: "writeFile",
				args: [gatewayPath, "hello gateway"],
			}),
			30_000,
			"gateway writeFile",
		);
		const gatewayContent = await waitForPromise(
			handle.action({ name: "readFile", args: [gatewayPath] }),
			30_000,
			"gateway readFile",
		);
		expect(bytesToString(gatewayContent)).toBe("hello gateway");

		const actorId = await waitForPromise(
			handle.resolve(),
			30_000,
			"resolve actor id",
		);
		expect(typeof actorId).toBe("string");
		expect(actorId.length).toBeGreaterThan(0);

		const conn = handle.connect();
		try {
			await waitForPromise(conn.ready, 30_000, "actor connection");
			expect(conn.actorId).toBe(actorId);

			const directHandle = client.getForId(INSPECTOR_ACTOR_NAME, actorId);
			const directPath = `/tmp/direct-${crypto.randomUUID()}.txt`;
			await waitForPromise(
				directHandle.action({
					name: "writeFile",
					args: [directPath, "hello actor id"],
				}),
				30_000,
				"direct actor-id writeFile",
			);
			const directContent = await waitForPromise(
				directHandle.action({ name: "readFile", args: [directPath] }),
				30_000,
				"direct actor-id readFile",
			);
			expect(bytesToString(directContent)).toBe("hello actor id");
		} finally {
			await conn.dispose();
		}
	}, 120_000);
});
