// @rivet-dev/agentos

export {
	SidecarProcessError,
	SidecarProcessExited,
	SidecarRejectedError,
	type SidecarRejectionDetail,
	SidecarSilenceTimeout,
} from "@rivet-dev/agentos-runtime-core/sidecar-errors";
export { AgentOs, AgentOsSidecar } from "./agent-os.js";
export {
	isPackageDescriptor,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	tryReadAgentosPackageManifest,
} from "./agentos-package.js";
export {
	CronManager,
	InvalidScheduleError,
	PastScheduleError,
	TimerScheduleDriver,
} from "./cron/index.js";
export {
	createHostDirBackend,
	hostDirMount,
	nodeModulesMount,
} from "./host-dir-mount.js";
export type {
	HostFunction,
	HostFunctionCollection,
	HostFunctionCollections,
	HostFunctionExample,
	HostFunctionSchemas,
	ResolvedHostFunctions,
} from "@rivet-dev/agentos-runtime-core/host-functions";
export {
	hostFunctionCommandName,
	hostFunctionDescription,
	resolveHostFunctions,
} from "@rivet-dev/agentos-runtime-core/host-functions";
export type * from "./language-execution.js";
export { createSnapshotExport } from "./layers.js";
export {
	agentOsLimitsSchema,
	agentOsOptionFieldSchemas,
	agentOsOptionsSchema,
	hostFunctionCollectionSchema,
	hostFunctionSchema,
	hostFunctionsSchema,
	mountConfigSchema,
	nativeMountConfigSchema,
	parseAgentOsOptions,
	permissionsSchema,
	rootFilesystemConfigSchema,
	sharedSidecarConfigSchema,
	sidecarConfigSchema,
	sidecarRuntimeConfigSchema,
} from "./options-schema.js";
export { defineSoftware } from "./packages.js";
export type {
	ExecOptions,
	ExecResult,
	ManagedProcess,
	ProcessInfo,
	ShellHandle,
	VirtualDirEntry,
	VirtualStat,
} from "./runtime.js";
export { KernelError } from "./runtime-compat.js";
export {
	createSandboxFs,
	createSandboxHostFunctions,
	getSandboxDisposeHooks,
	resolveSandboxOptions,
} from "./sandbox.js";
export type * from "./types.js";
