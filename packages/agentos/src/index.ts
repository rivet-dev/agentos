import { setup as rivetkitSetup } from "rivetkit";

const AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULT = 512 * 1024 * 1024;

/**
 * RivetKit setup with the direct actor SQLite UDS enabled. The UDS is consumed
 * by the AgentOS sidecar; filesystem SQL never crosses the JavaScript layer.
 */
export const setup: typeof rivetkitSetup = ((
	input: Parameters<typeof rivetkitSetup>[0],
) =>
	rivetkitSetup({
		...input,
		maxIncomingMessageSize: AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULT,
		maxOutgoingMessageSize: AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULT,
		experimentalActorUds: true,
	} as Parameters<typeof rivetkitSetup>[0])) as typeof rivetkitSetup;

export type {
	AgentOsOptions,
	DirEntry,
	DynamicMountDescriptor,
	ExportRootFilesystemOptions,
	HttpRequest,
	HttpResponse,
	MountInfo,
	NodeModulesMountConfig,
	PackageDescriptor,
	ProcessExit,
	ProcessOutput,
	PromptResult,
	ReaddirEntry,
	RootSnapshotExport,
	SessionInfo,
	ShellData,
	ShellExit,
} from "@rivet-dev/agentos-core";
export { defineSoftware, nodeModulesMount } from "@rivet-dev/agentos-core";
export type {
	AgentOsActorConfigInput as AgentOSActorConfigInput,
	AgentOsActorConfigInput as AgentOSConfigInput,
} from "./actor.js";
export {
	type AgentOsActions,
	type AgentOsActorConfigInput,
	type AgentOsActorExtras,
	type AgentOsEventHooks,
	createAgentOS,
	createAgentOS as agentOS,
	createAgentOsActions,
} from "./actor.js";
export type {
	AgentOsEvents,
	ProcessExitPayload,
	ProcessOutputPayload,
	RuntimeAgentExit,
	RuntimeHealth,
	RuntimeLimitWarning,
	RuntimeSidecarInfo,
	RuntimeStderrLine,
	SerializableCronAction,
	SerializableCronEvent,
	SerializableCronJobInfo,
	SerializableCronJobOptions,
	ShellDataPayload,
	ShellExitPayload,
	VmBootedPayload,
	VmShutdownPayload,
} from "./types.js";
