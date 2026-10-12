// @rivet-dev/agentos

export { AgentOs, AgentOsSidecar } from "./agent-os.js";
export {
	isPackageDescriptor,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	tryReadAgentosPackageManifest,
} from "./agentos-package.js";
// Low-level VM, protocol, and sidecar client APIs.
export * from "./binary.js";
export * from "./bytes.js";
export * from "./callbacks.js";
export * from "./correlation.js";
export {
	CronManager,
	InvalidScheduleError,
	PastScheduleError,
	TimerScheduleDriver,
} from "./cron/index.js";
export * from "./descriptors.js";
export { SidecarEventBufferOverflow } from "./event-buffer.js";
export * from "./ext.js";
export * from "./filesystem.js";
export * from "./frame-payload-codec.js";
export * from "./frame-rpc.js";
export * from "./frame-stream.js";
export * from "./framing.js";
export * as protocol from "./generated-protocol.js";
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
// Hosted transport is loaded only when createAgentOsClient is called.
export * from "./hosted-client.js";
export * from "./json.js";
export type * from "./language-execution.js";
export { createSnapshotExport } from "./layers.js";
export * from "./node-runtime.js";
export * from "./node-runtime-options-schema.js";
export * from "./numbers.js";
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
export * from "./permissions.js";
export * from "./process.js";
export * from "./protocol-client.js";
export * from "./protocol-frames.js";
export * from "./request-payloads.js";
export * from "./response-payloads.js";
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
export * from "./sidecar-client.js";
export * from "./sidecar-errors.js";
export {
	SidecarFrameTooLarge,
	SidecarProcessError,
	SidecarProcessExited,
	SidecarRejectedError,
	type SidecarRejectionDetail,
	SidecarSilenceTimeout,
} from "./sidecar-errors.js";
export type {
	ResolvedSidecarSpawnOptions,
	SidecarSpawnOptions,
} from "./sidecar-process.js";
export {
	registerSidecarProcessSpawnFactory,
	SidecarProcess,
} from "./sidecar-process.js";
export {
	DEFAULT_SOFTWARE_REGISTRY_URL,
	resolveRegistrySoftware,
	SOFTWARE_REGISTRY_URL_ENV,
} from "./software-registry.js";
export * from "./state.js";
export * from "./stdio-client.js";
export type * from "./types.js";
