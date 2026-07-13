import { describe, expect, test } from "vitest";
import {
	AgentOs,
	AgentOsSidecar,
	CronManager,
	KernelError,
	InvalidScheduleError,
	PastScheduleError,
	agentOsLimitsSchema,
	agentOsOptionsSchema,
	createHostDirBackend,
	createInMemoryFileSystem,
	createInMemoryLayerStore,
	createSnapshotExport,
	defineSoftware,
	isPackageDescriptor,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	hostTool,
	hostToolSchema,
	isAcpTimeoutErrorData,
	isUnknownSessionErrorData,
	mountConfigSchema,
	nodeModulesMount,
	parseAgentOsOptions,
	rootFilesystemConfigSchema,
	toolKit,
	toolKitSchema,
	type AcpTimeoutErrorData,
	type AgentOsLimits,
	type ExecOptions,
	type HostDirMountPluginConfig,
	type JsonRpcErrorData,
	type KernelExecOptions,
	type KernelExecResult,
	type KernelSpawnOptions,
	type MountConfigJsonPrimitive,
	type NodeModulesMountConfig,
	type OpenShellOptions,
	type PromptCapabilities,
	type PromptResult,
	type ResumeSessionOptions,
	type ResumeSessionResult,
	type StdioChannel,
	type TimingMitigation,
	type UnknownSessionErrorData,
} from "../src/index.js";

describe("root public API exports", () => {
	test("re-exports the main public value surface from the root entrypoint", () => {
		expect(AgentOs).toBeTypeOf("function");
		expect(AgentOsSidecar).toBeTypeOf("function");
		expect(CronManager).toBeTypeOf("function");
		expect(createHostDirBackend).toBeTypeOf("function");
		expect(hostTool).toBeTypeOf("function");
		expect(toolKit).toBeTypeOf("function");
		expect(agentOsLimitsSchema.safeParse({}).success).toBe(true);
		expect(
			agentOsOptionsSchema.safeParse({ defaultSoftware: false }).success,
		).toBe(true);
		expect(hostToolSchema).toBeTypeOf("object");
		expect(toolKitSchema).toBeTypeOf("object");
		expect(mountConfigSchema).toBeTypeOf("object");
		expect(rootFilesystemConfigSchema).toBeTypeOf("object");
		expect(parseAgentOsOptions({ defaultSoftware: false })).toEqual({
			defaultSoftware: false,
		});
		expect(createInMemoryFileSystem).toBeTypeOf("function");
		expect(KernelError).toBeTypeOf("function");
		expect(createInMemoryLayerStore).toBeTypeOf("function");
		expect(createSnapshotExport).toBeTypeOf("function");
		// Package dirs are the public software descriptor.
		expect(defineSoftware("/opt/pkg")).toBe("/opt/pkg");
		expect(isPackageDescriptor).toBeTypeOf("function");
		expect(OPT_AGENTOS_ROOT).toBe("/opt/agentos");
		expect(OPT_AGENTOS_BIN).toBe("/opt/agentos/bin");
	});

	test("re-exports current public SDK types from the root entrypoint", () => {
		void (null as AcpTimeoutErrorData | null);
		void (null as AgentOsLimits | null);
		void (null as ExecOptions | null);
		void (null as HostDirMountPluginConfig | null);
		void (null as JsonRpcErrorData | null);
		void (null as KernelExecOptions | null);
		void (null as KernelExecResult | null);
		void (null as KernelSpawnOptions | null);
		void (null as MountConfigJsonPrimitive | null);
		void (null as NodeModulesMountConfig | null);
		void (null as OpenShellOptions | null);
		void (null as PromptCapabilities | null);
		void (null as PromptResult | null);
		void (null as ResumeSessionOptions | null);
		void (null as ResumeSessionResult | null);
		void (null as StdioChannel | null);
		void (null as TimingMitigation | null);
		void (null as UnknownSessionErrorData | null);

		expect(true).toBe(true);
	});

	test("re-exports nodeModulesMount helper from the root entrypoint", () => {
		const mount = nodeModulesMount("/host/project/node_modules");
		expect(mount.path).toBe("/root/node_modules");
		expect(mount.readOnly).toBeUndefined();
		expect(mount.plugin.id).toBe("host_dir");
		expect(mount.plugin.config.hostPath).toBe("/host/project/node_modules");
		expect(mount.plugin.config.readOnly).toBeUndefined();

		const writable = nodeModulesMount("/host/project/node_modules", {
			readOnly: false,
		});
		expect(writable.readOnly).toBe(false);
		expect(writable.plugin.config.readOnly).toBe(false);
	});

	test("re-exports ACP timeout diagnostics helper from the root entrypoint", () => {
		const timeout: AcpTimeoutErrorData = {
			kind: "acp_timeout",
			method: "session/prompt",
			id: 7,
			timeoutMs: 5000,
			recentActivity: ["waiting for adapter"],
		};

		expect(isAcpTimeoutErrorData(timeout)).toBe(true);
		expect(isAcpTimeoutErrorData({ kind: "other" })).toBe(false);
	});

	test("re-exports unknown-session discriminator helper from the root entrypoint", () => {
		const unknownSession: UnknownSessionErrorData = {
			kind: "unknown_session",
			sessionId: "sess-123",
		};

		expect(isUnknownSessionErrorData(unknownSession)).toBe(true);
		// `sessionId` is optional — the discriminator is `kind` alone, matching the
		// sidecar's normalized `{kind}`-only shape.
		expect(isUnknownSessionErrorData({ kind: "unknown_session" })).toBe(true);
		expect(
			isUnknownSessionErrorData({ kind: "unknown_session", sessionId: 5 }),
		).toBe(false);
		expect(isUnknownSessionErrorData({ kind: "other" })).toBe(false);
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
