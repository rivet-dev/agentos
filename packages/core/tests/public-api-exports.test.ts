import { describe, expect, test } from "vitest";
import * as publicApi from "../src/index.js";
import {
	AgentOs,
	AgentOsSidecar,
	CronManager,
	KernelError,
	MAX_BINDING_DESCRIPTION_LENGTH,
	InvalidScheduleError,
	PastScheduleError,
	TimerScheduleDriver,
	agentOsLimitsSchema,
	agentOsOptionsSchema,
	createHostDirBackend,
	createSnapshotExport,
	defineSoftware,
	isPackageDescriptor,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	binding,
	bindingSchema,
	mountConfigSchema,
	nodeModulesMount,
	parseAgentOsOptions,
	rootFilesystemConfigSchema,
	bindings,
	bindingsSchema,
	validateBindings,
	type AgentOsLimits,
	type ExecOptions,
	type HostDirMountPluginConfig,
	type KernelExecOptions,
	type KernelExecResult,
	type KernelSpawnOptions,
	type MountConfigJsonPrimitive,
	type NodeModulesMountConfig,
	type OpenShellOptions,
	type OpenSessionInput,
	type PermissionResponse,
	type PromptResult,
	type SessionCapabilities,
	type SessionInfo,
	type SessionStreamEntry,
	type StdioChannel,
	type TimingMitigation,
} from "../src/index.js";

describe("root public API exports", () => {
	test("does not expose client-owned in-memory filesystem factories", () => {
		expect(publicApi).not.toHaveProperty("createInMemoryFileSystem");
		expect(publicApi).not.toHaveProperty("createInMemoryLayerStore");
	});
	test("re-exports the main public value surface from the root entrypoint", () => {
		expect(AgentOs).toBeTypeOf("function");
		expect(AgentOsSidecar).toBeTypeOf("function");
		expect(CronManager).toBeTypeOf("function");
		expect(TimerScheduleDriver).toBeTypeOf("function");
		expect(createHostDirBackend).toBeTypeOf("function");
		expect(binding).toBeTypeOf("function");
		expect(bindings).toBeTypeOf("function");
		expect(validateBindings).toBeTypeOf("function");
		expect(MAX_BINDING_DESCRIPTION_LENGTH).toBeGreaterThan(0);
		expect(agentOsLimitsSchema.safeParse({}).success).toBe(true);
		expect(
			agentOsLimitsSchema.safeParse({
				process: {
					maxSpawnFileActions: 4096,
					maxSpawnFileActionBytes: 1024 * 1024,
					pendingStdinBytes: 1024,
					pendingEventCount: 16,
					pendingEventBytes: 4096,
				},
			}).success,
		).toBe(true);
		expect(
			agentOsLimitsSchema.safeParse({
				process: { pendingEventCount: 0 },
			}).success,
		).toBe(false);
		expect(
			agentOsLimitsSchema.safeParse({
				process: { maxSpawnFileActions: 0 },
			}).success,
		).toBe(false);
		expect(
			agentOsLimitsSchema.safeParse({
				process: { maxSpawnFileActionBytes: 0 },
			}).success,
		).toBe(false);
		expect(
			agentOsOptionsSchema.safeParse({ defaultSoftware: false }).success,
		).toBe(true);
		expect(bindingSchema).toBeTypeOf("object");
		expect(bindingsSchema).toBeTypeOf("object");
		expect(mountConfigSchema).toBeTypeOf("object");
		expect(rootFilesystemConfigSchema).toBeTypeOf("object");
		expect(parseAgentOsOptions({ defaultSoftware: false })).toEqual({
			defaultSoftware: false,
		});
		expect(KernelError).toBeTypeOf("function");
		expect(createSnapshotExport).toBeTypeOf("function");
		// Package dirs are the public software descriptor.
		expect(defineSoftware("/opt/pkg")).toBe("/opt/pkg");
		expect(isPackageDescriptor).toBeTypeOf("function");
		expect(OPT_AGENTOS_ROOT).toBe("/opt/agentos");
		expect(OPT_AGENTOS_BIN).toBe("/opt/agentos/bin");
	});

	test("re-exports current public SDK types from the root entrypoint", () => {
		void (null as AgentOsLimits | null);
		void (null as ExecOptions | null);
		void (null as HostDirMountPluginConfig | null);
		void (null as KernelExecOptions | null);
		void (null as KernelExecResult | null);
		void (null as KernelSpawnOptions | null);
		void (null as MountConfigJsonPrimitive | null);
		void (null as NodeModulesMountConfig | null);
		void (null as OpenShellOptions | null);
		void (null as OpenSessionInput | null);
		void (null as PermissionResponse | null);
		void (null as PromptResult | null);
		void (null as SessionCapabilities | null);
		void (null as SessionInfo | null);
		void (null as SessionStreamEntry | null);
		void (null as StdioChannel | null);
		void (null as TimingMitigation | null);

		expect(true).toBe(true);
	});

	test("re-exports nodeModulesMount helper from the root entrypoint", () => {
		const mount = nodeModulesMount("/host/project/node_modules");
		expect(mount.path).toBe("/root/node_modules");
		expect(mount.readOnly).toBe(true);
		expect(mount.plugin.id).toBe("host_dir");
		expect(mount.plugin.config.hostPath).toBe("/host/project/node_modules");
		expect(mount.plugin.config.readOnly).toBe(true);

		const writable = nodeModulesMount("/host/project/node_modules", {
			readOnly: false,
		});
		expect(writable.readOnly).toBe(false);
		expect(writable.plugin.config.readOnly).toBe(false);
	});

	test("re-exports cron scheduling errors from the root entrypoint", () => {
		expect(new InvalidScheduleError("tomorrow").name).toBe(
			"InvalidScheduleError",
		);
		expect(new PastScheduleError("2020-01-01T00:00:00Z").name).toBe(
			"PastScheduleError",
		);
	});
});
