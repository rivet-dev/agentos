// @rivet-dev/agentos

export { AgentOs, AgentOsSidecar } from "./agent-os.js";
export { AGENT_CONFIGS } from "./agents.js";
export {
	CronManager,
	InvalidScheduleError,
	PastScheduleError,
	TimerScheduleDriver,
} from "./cron/index.js";
export { createHostDirBackend, nodeModulesMount } from "./host-dir-mount.js";
export {
	binding,
	bindingGroup,
	MAX_BINDING_DESCRIPTION_LENGTH,
	normalizeBindingGroup,
	normalizeBindingGroups,
	validateBindings,
} from "./host-bindings.js";
export {
	agentOsLimitsSchema,
	agentOsOptionFieldSchemas,
	agentOsOptionsSchema,
	bindingGroupSchema,
	bindingSchema,
	mountConfigSchema,
	nativeMountConfigSchema,
	parseAgentOsOptions,
	permissionsSchema,
	rootFilesystemConfigSchema,
	sharedSidecarConfigSchema,
	sidecarConfigSchema,
} from "./options-schema.js";
export {
	createSandboxBindings,
	createSandboxFs,
	getSandboxDisposeHooks,
	resolveSandboxOptions,
} from "./sandbox.js";
export {
	createInMemoryLayerStore,
	createSnapshotExport,
} from "./layers.js";
export { defineSoftware } from "./packages.js";
export {
	isPackageDescriptor,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	tryReadAgentosPackageManifest,
} from "./agentos-package.js";
export {
	isAcpTimeoutErrorData,
	isUnknownSessionErrorData,
} from "./json-rpc.js";
export { createInMemoryFileSystem, KernelError } from "./runtime-compat.js";
export type {
	ExecOptions,
	ExecResult,
	ManagedProcess,
	ProcessInfo,
	ShellHandle,
	VirtualDirEntry,
	VirtualStat,
} from "./runtime.js";
export type * from "./types.js";
