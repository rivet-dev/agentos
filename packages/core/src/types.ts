export type {
	AgentCapabilities,
	AgentInfo,
	AgentOsCreateSidecarOptions,
	AgentOsLimits,
	AgentOsOptions,
	AgentOsSharedSidecarOptions,
	AgentOsSidecarConfig,
	AgentOsSidecarDescription,
	AgentExitEvent,
	AgentExitHandler,
	AgentRestartOutcome,
	AgentStderrEvent,
	AgentStderrHandler,
	AgentRegistryEntry,
	CreateSessionOptions,
	DirEntry,
	LimitWarning,
	LimitWarningHandler,
	McpServerConfig,
	McpServerConfigLocal,
	McpServerConfigRemote,
	MountConfig,
	MountConfigJsonObject,
	MountConfigJsonPrimitive,
	MountConfigJsonValue,
	NativeMountConfig,
	NativeMountPluginDescriptor,
	OverlayMountConfig,
	PermissionReply,
	PermissionRequest,
	PermissionRequestHandler,
	PlainMountConfig,
	PromptResult,
	ReaddirRecursiveOptions,
	ResumeSessionOptions,
	ResumeSessionResult,
	RootFilesystemConfig,
	RootLowerInput,
	SessionConfigOption,
	SessionEventHandler,
	SessionInfo,
	SessionInitData,
	SessionMode,
	SessionModeState,
	SpawnedProcessInfo,
} from "./agent-os.js";
/**
 * An agent type id — the `name` of an `/opt/agentos` agent package manifest
 * (e.g. `"pi"`, `"claude"`). Agents are resolved by the SIDECAR from the projected
 * package manifest (`/opt/agentos/<name>/current/agentos-package.json`); the client
 * passes only the name, so any manifest `name` is a valid agent type.
 */
export type AgentType = string;
export type {
	CronAction,
	CronEvent,
	CronEventHandler,
	CronJob,
	CronJobInfo,
	CronJobOptions,
} from "./cron/index.js";
export type {
	HostDirBackendOptions,
	HostDirMountPluginConfig,
	NodeModulesMountConfig,
} from "./host-dir-mount.js";
export type { HostTool, ToolExample, ToolKit } from "./host-tools.js";
export type {
	AcpTimeoutErrorData,
	JsonRpcError,
	JsonRpcErrorData,
	JsonRpcNotification,
	JsonRpcRequest,
	JsonRpcResponse,
	UnknownSessionErrorData,
} from "./json-rpc.js";
export type {
	FilesystemSnapshotExport,
	LayerHandle,
	LayerStore,
	OverlayFilesystemMode,
	RootSnapshotExport,
	SnapshotImportSource,
	SnapshotLayerHandle,
	WritableLayerHandle,
} from "./layers.js";
export type { SoftwareInput } from "./packages.js";
export type {
	AgentBlock,
	PackageDescriptor,
	PackageRef,
	SoftwarePackageRef,
} from "./agentos-package.js";
export type {
	ChildProcessPermissions,
	EnvPermissions,
	ExecOptions,
	ExecResult,
	FsPermissionRule,
	FsPermissions,
	KernelExecOptions,
	KernelExecResult,
	KernelSpawnOptions,
	NetworkAccessRequest,
	NetworkPermissions,
	OpenShellOptions,
	PatternPermissionRule,
	PermissionDecision,
	PermissionMode,
	Permissions,
	ProcessInfo,
	RulePermissions,
	StdioChannel,
	VirtualDirEntry,
	VirtualFileSystem,
	VirtualStat,
} from "./runtime.js";
export type { PromptCapabilities } from "./agent-session-types.js";
