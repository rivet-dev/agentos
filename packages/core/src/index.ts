// @rivet-dev/agentos

export {
	SidecarProcessError,
	SidecarProcessExited,
	SidecarRejectedError,
	type SidecarRejectionDetail,
	SidecarSilenceTimeout,
} from "./sidecar-errors.js";
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
} from "./host-functions.js";
export {
	hostFunctionCommandName,
	hostFunctionDescription,
	resolveHostFunctions,
} from "./host-functions.js";
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

// Low-level VM, protocol, and sidecar client APIs.
export * from "./binary.js";
export * from "./bytes.js";
export * from "./callbacks.js";
export * from "./correlation.js";
export * from "./descriptors.js";
export * from "./ext.js";
export * from "./frame-payload-codec.js";
export * from "./frame-rpc.js";
export * from "./frame-stream.js";
export * from "./filesystem.js";
export * from "./framing.js";
export * from "./json.js";
export * from "./stdio-client.js";
export * from "./node-runtime.js";
export * from "./node-runtime-options-schema.js";
export * from "./numbers.js";
export * from "./permissions.js";
export * from "./process.js";
export * from "./protocol-client.js";
export * from "./protocol-frames.js";
export * from "./request-payloads.js";
export * from "./response-payloads.js";
export * from "./sidecar-client.js";
export * from "./sidecar-errors.js";
export {
	registerSidecarProcessSpawnFactory,
	SidecarProcess,
} from "./sidecar-process.js";
export type {
	ResolvedSidecarSpawnOptions,
	SidecarSpawnOptions,
} from "./sidecar-process.js";
export * from "./state.js";
export * as protocol from "./generated-protocol.js";
