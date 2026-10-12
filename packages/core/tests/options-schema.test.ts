import { describe, expect, test } from "vitest";
import { AgentOs, agentOsOptionsSchema } from "../src/index.js";
import {
	getSandboxDisposeHooks,
	resolveSandboxOptions,
} from "../src/sandbox.js";

describe("AgentOsOptions validation", () => {
	test("accepts a complete initial environment including an explicit empty map", () => {
		expect(agentOsOptionsSchema.safeParse({ environment: {} }).success).toBe(
			true,
		);
		expect(
			agentOsOptionsSchema.safeParse({
				environment: { EMPTY: "", PATH: "/opt/agentos/bin" },
			}).success,
		).toBe(true);
		expect(
			agentOsOptionsSchema.safeParse({ environment: { PORT: 3000 } }).success,
		).toBe(false);
	});

	test("accepts the temporary local SQLite descriptor", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				database: {
					type: "sqlite_file",
					path: "/tmp/agentos.sqlite",
				},
			}).success,
		).toBe(true);
	});

	test("accepts a declarative sidecar-native root", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				rootFilesystem: {
					type: "native",
					plugin: {
						id: "chunked_sqlite",
						config: { namespace: "root" },
					},
				},
			}).success,
		).toBe(true);
	});

	test("rejects unknown top-level options before booting a VM", async () => {
		await expect(
			AgentOs.create({
				onSessionEvent: () => {},
			} as never),
		).rejects.toThrow(/onSessionEvent/);
	});

	test("rejects unknown nested permission fields", () => {
		expect(() =>
			agentOsOptionsSchema.parse({
				permissions: {
					filesystem: "allow",
				},
			}),
		).toThrow(/filesystem/);
	});

	test("accepts the distinct WASM CPU fields and rejects removed aliases", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				limits: {
					wasm: {
						activeCpuTimeLimitMs: 30_000,
						wallClockLimitMs: 45_000,
						deterministicFuel: 1_000_000,
					},
				},
			}).success,
		).toBe(true);
		expect(() =>
			agentOsOptionsSchema.parse({
				limits: { resources: { maxWasmFuel: 1 } },
			}),
		).toThrow(/maxWasmFuel/);
		expect(() =>
			agentOsOptionsSchema.parse({
				limits: { wasm: { runnerCpuTimeLimitMs: 1 } },
			}),
		).toThrow(/runnerCpuTimeLimitMs/);
	});

	test("accepts distinct per-process and per-VM WASM thread limits", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				limits: {
					wasm: { maxThreads: 8, maxConcurrentThreads: 32 },
				},
			}).success,
		).toBe(true);
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { wasm: { maxConcurrentThreads: 0 } },
			}).success,
		).toBe(false);
	});

	test("accepts only supported VM-wide standalone WASM backends", () => {
		for (const wasmBackend of ["v8", "wasmtime", "wasmtime-threads"] as const) {
			expect(agentOsOptionsSchema.safeParse({ wasmBackend }).success).toBe(
				true,
			);
		}
		expect(
			agentOsOptionsSchema.safeParse({ wasmBackend: "automatic" }).success,
		).toBe(false);
	});

	test("accepts only sidecar-owned VM defaults profiles", () => {
		for (const defaultsProfile of ["agent_os", "secure"] as const) {
			expect(agentOsOptionsSchema.safeParse({ defaultsProfile }).success).toBe(
				true,
			);
		}
		expect(
			agentOsOptionsSchema.safeParse({ defaultsProfile: "custom" }).success,
		).toBe(false);
	});

	test("bounds and materializes Linux account records", () => {
		const exactPasswdRecord = {
			uid: 0,
			gid: 0,
			username: "u",
			homedir: "/",
			shell: "/",
			gecos: "x".repeat(4083),
		};
		expect(
			agentOsOptionsSchema.safeParse({ user: exactPasswdRecord }).success,
		).toBe(true);
		expect(
			agentOsOptionsSchema.safeParse({
				user: { ...exactPasswdRecord, gecos: "😀".repeat(1021) },
			}).success,
		).toBe(false);
		expect(
			agentOsOptionsSchema.safeParse({
				user: {
					uid: 0,
					gid: 0,
					username: "root",
					supplementaryGids: [44],
					groups: [{ gid: 99, name: "group44", members: [] }],
				},
			}).success,
		).toBe(false);
		expect(
			agentOsOptionsSchema.safeParse({
				user: {
					groups: [
						{
							gid: 7,
							name: "g",
							members: Array.from({ length: 257 }, (_, index) => `m${index}`),
						},
					],
				},
			}).success,
		).toBe(false);
	});

	test("rejects create option factories on the one-shot core constructor", () => {
		expect(() =>
			agentOsOptionsSchema.parse({
				createOptions: () => ({}),
			}),
		).toThrow(/createOptions/);
	});

	test("accepts hostFunctions as the public name for host-function collections", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				hostFunctions: { weather: {} },
			}).success,
		).toBe(true);
	});

	test("accepts a sandbox provider as a public VM option", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				sandbox: { provider: { start: async () => ({}) } },
			}).success,
		).toBe(true);
	});

	test("uses the sidecar wire name for the per-VM host-function limit", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { hostFunctions: { maxRegisteredFunctionsPerVm: 256 } },
			}).success,
		).toBe(true);
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { hostFunctions: { maxRegisteredCollectionsPerVm: 256 } },
			}).success,
		).toBe(false);
	});

	test("validates execution retention limits as positive safe integers", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				limits: {
					execution: {
						completedTtlMs: 300_000,
						maxCompletedExecutions: 1_024,
						liveExecutionWarningThreshold: 64,
					},
				},
			}).success,
		).toBe(true);
		for (const value of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
			expect(
				agentOsOptionsSchema.safeParse({
					limits: { execution: { completedTtlMs: value } },
				}).success,
			).toBe(false);
		}
	});

	test("accepts optional TLS/execution fields and validates TLS bytes", () => {
		for (const limits of [
			{ tls: {}, execution: {} },
			{ tls: { maxBufferedBytes: 2048 } },
		]) {
			expect(agentOsOptionsSchema.parse({ limits }).limits).toEqual(limits);
		}
		for (const maxBufferedBytes of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
			expect(
				agentOsOptionsSchema.safeParse({
					limits: { tls: { maxBufferedBytes } },
				}).success,
			).toBe(false);
		}
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { tls: { max_buffered_bytes: 2048 } },
			}).success,
		).toBe(false);
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { execution: { completedTtl: 60_000 } },
			}).success,
		).toBe(false);
	});

	test("accepts package mount limits and rejects invalid or unknown fields", () => {
		for (const packages of [{}, { maxMounts: 8192 }]) {
			expect(
				agentOsOptionsSchema.safeParse({
					limits: { agentosPackages: packages },
				}).success,
			).toBe(true);
		}
		for (const maxMounts of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
			expect(
				agentOsOptionsSchema.safeParse({
					limits: { agentosPackages: { maxMounts } },
				}).success,
			).toBe(false);
		}
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { agentosPackages: { maxMount: 8 } },
			}).success,
		).toBe(false);
	});
	test("provider sandbox starts a client and owns disposal", async () => {
		let disposed = false;
		const client = {
			baseUrl: "http://127.0.0.1:1234",
			dispose: () => {
				disposed = true;
			},
		} as never;

		const options = await resolveSandboxOptions({
			sandbox: {
				provider: {
					start: async () => client,
				},
			},
		} as never);
		expect(options).not.toHaveProperty("sandbox");
		expect(options.mounts?.[0]?.path).toBe("/mnt/sandbox");
		expect(Object.keys(options.hostFunctions ?? {})).toContain("sandbox");

		for (const hook of getSandboxDisposeHooks(options)) {
			await hook();
		}
		expect(disposed).toBe(true);
	});

	test("advanced sandbox client leaves disposal manual by default", async () => {
		const client = { baseUrl: "http://127.0.0.1:1234" } as never;
		const options = await resolveSandboxOptions({
			sandbox: {
				client,
				mountPath: "/work",
			},
		} as never);
		expect(options.mounts?.[0]?.path).toBe("/work");
		expect(getSandboxDisposeHooks(options)).toHaveLength(0);
	});

	test("disposes a provider client when sandbox expansion fails", async () => {
		let disposed = 0;
		await expect(
			resolveSandboxOptions({
				sandbox: {
					provider: {
						start: async () => ({
							dispose: () => {
								disposed += 1;
							},
						}),
					},
				},
			} as never),
		).rejects.toThrow(/serializable baseUrl/);
		expect(disposed).toBe(1);
	});

	test("does not start a provider when VM option validation fails", async () => {
		let started = 0;
		let disposed = 0;
		await expect(
			AgentOs.create({
				defaultSoftware: false,
				sandbox: {
					provider: {
						start: async () => {
							started += 1;
							return {
								baseUrl: "http://127.0.0.1:1234",
								dispose: () => {
									disposed += 1;
								},
							} as never;
						},
					},
				},
				hostFunctions: { INVALID_NAME: {} },
			}),
		).rejects.toThrow(/must be alphanumeric, written in camelCase/);
		expect(started).toBe(0);
		expect(disposed).toBe(0);
	});

	test("rejects removed sandbox mount and hostFunction toggles", async () => {
		const client = { baseUrl: "http://127.0.0.1:1234" } as never;
		await expect(
			resolveSandboxOptions({
				sandbox: {
					client,
					mount: false,
				} as never,
			} as never),
		).rejects.toThrow(/sandbox\.mount has been removed/);

		await expect(
			resolveSandboxOptions({
				sandbox: {
					client,
					hostFunctions: false,
				} as never,
			} as never),
		).rejects.toThrow(/sandbox\.hostFunctions has been removed/);
	});

	test("rejects old sandbox path option names", async () => {
		const client = { baseUrl: "http://127.0.0.1:1234" } as never;
		await expect(
			resolveSandboxOptions({
				sandbox: {
					client,
					basePath: "/app",
				} as never,
			} as never),
		).rejects.toThrow(/sandbox\.basePath has been removed/);
	});
});
