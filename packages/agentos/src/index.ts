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
	CreateSessionOptions,
	DirEntry,
	NodeModulesMountConfig,
	PromptResult,
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
	createAgentOS,
	createAgentOS as agentOS,
	createAgentOsActions,
	type VmFetchOptions,
	type VmFetchResponse,
} from "./actor.js";
export type {
	AgentCrashedPayload,
	AgentOsEvents,
	CronEventPayload,
	PermissionRequestPayload,
	PersistedSessionEvent,
	PersistedSessionRecord,
	ProcessExitPayload,
	ProcessOutputPayload,
	SerializableCronAction,
	SerializableCronEvent,
	SerializableCronJobInfo,
	SerializableCronJobOptions,
	SessionEventPayload,
	SessionRecord,
	ShellDataPayload,
	VmBootedPayload,
	VmShutdownPayload,
} from "./types.js";
