// @rivet-dev/agentos

export { AgentOs, AgentOsSidecar } from "./agent-os.js";
export type * from "./language-execution.js";
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
export {
	hostFunction,
	MAX_HOST_FUNCTION_DESCRIPTION_LENGTH,
	hostFunctions,
	validateHostFunctions,
} from "./host-functions.js";
export type { HostFunction, HostFunctionExample, HostFunctions } from "./host-functions.js";
export {
	agentOsLimitsSchema,
	agentOsOptionFieldSchemas,
	agentOsOptionsSchema,
	hostFunctionSchema,
	mountConfigSchema,
	nativeMountConfigSchema,
	parseAgentOsOptions,
	permissionsSchema,
	rootFilesystemConfigSchema,
	sharedSidecarConfigSchema,
	sidecarConfigSchema,
	hostFunctionsSchema,
	sidecarRuntimeConfigSchema,
} from "./options-schema.js";
export { createSnapshotExport } from "./layers.js";
export { defineSoftware } from "./packages.js";
export {
	isPackageDescriptor,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	tryReadAgentosPackageManifest,
} from "./agentos-package.js";
export { KernelError } from "./runtime-compat.js";
export {
	SidecarProcessError,
	SidecarProcessExited,
	SidecarRejectedError,
	type SidecarRejectionDetail,
	SidecarSilenceTimeout,
} from "@rivet-dev/agentos-runtime-core/sidecar-errors";
export type {
	ExecOptions,
	ExecResult,
	ManagedProcess,
	ProcessInfo,
	ShellHandle,
	VirtualDirEntry,
	VirtualStat,
} from "./runtime.js";
export {
	createSandboxHostFunctions,
	createSandboxFs,
	getSandboxDisposeHooks,
	resolveSandboxOptions,
} from "./sandbox.js";
export type * from "./types.js";
