// Rust-backed agent-os actor surface (native actor plugin / cdylib).
//
// Only the `agentOS()` definition function, the config schema, the
// `nodeModulesMount` helper, the plugin-path resolver, and the public domain
// types are exported. All actor lifecycle + action dispatch live in the Rust
// plugin (`crates/agentos-actor-plugin`), loaded by RivetKit via the generic
// native-plugin ABI.

import type {
	AgentOsOptions,
	JsonRpcNotification,
	PermissionRequest,
} from "@rivet-dev/agentos-core";
import {
	type AgentOsActorDefinition,
	createAgentOS,
} from "./actor.js";
import type {
	AgentOsActorConfigInput,
	NativeAgentOsOptions,
} from "./config.js";

import { setup as rivetkitSetup } from "rivetkit";

// 512 MiB — large agent prompts/results cross the actor websocket transport as
// single messages; RivetKit's registry-level defaults (maxIncomingMessageSize
// 64 KiB, maxOutgoingMessageSize 1 MiB) truncate real payloads. Still a finite
// bound per the limits-and-observability policy.
const AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULT = 512 * 1024 * 1024;

/**
 * `setup()` wrapper that defaults the registry transport message-size caps to
 * never-hit-by-normal-use values for AgentOS apps, mirroring the per-actor crank
 * in `DEFAULT_AGENTOS_ACTOR_OPTIONS`. Any caller-supplied value wins, so an app
 * can still tighten the bound when it wants one.
 */
export const setup: typeof rivetkitSetup = ((
	input: Parameters<typeof rivetkitSetup>[0],
) =>
	rivetkitSetup({
		maxIncomingMessageSize: AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULT,
		maxOutgoingMessageSize: AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULT,
		...input,
	})) as typeof rivetkitSetup;

export {
	buildConfigJson,
	type NodeModulesMountConfig,
	nodeModulesMount,
} from "./actor.js";
export {
	type AgentOsActorConfig,
	type AgentOsActorConfigInput,
	type NativeAgentOsOptions,
	agentOsActorConfigSchema,
	nativeAgentOsOptionsSchema,
} from "./config.js";
export { getPluginPath } from "./plugin-binary.js";

// Re-export the software-definition helper so custom agents/tools/commands can
// be defined without importing @rivet-dev/agentos-core directly.
export { defineSoftware } from "@rivet-dev/agentos-core";

export type {
	AgentOsActionContext,
	AgentOsActorState,
	AgentOsActorVars,
	AgentOsEvents,
	CronEventPayload,
	PermissionRequestPayload,
	PersistedSessionEvent,
	PersistedSessionRecord,
	ProcessExitPayload,
	ProcessOutputPayload,
	PromptResult,
	SerializableCronAction,
	SerializableCronJobInfo,
	SerializableCronJobOptions,
	SessionEventPayload,
	SessionRecord,
	ShellDataPayload,
	VmBootedPayload,
	VmShutdownPayload,
} from "./types.js";
export type {
	AgentOsActions,
	CreateSessionOptions,
	DirEntry,
	ReadFileResult,
	ScheduledCronJob,
	SignedPreviewUrl,
	SpawnedProcess,
	VmFetchOptions,
	VmFetchResponse,
	WriteFileResult,
} from "./actor-actions.js";

export type AgentOSActorConfigInput<TConnParams = undefined> =
	NativeAgentOsOptions &
		Omit<AgentOsActorConfigInput<TConnParams>, "options">;

export type AgentOSConfigInput<TConnParams = undefined> = AgentOsOptions & {
	preview?: AgentOsActorConfigInput<TConnParams>["preview"];
	onBeforeConnect?: AgentOsActorConfigInput<TConnParams>["onBeforeConnect"];
	onSessionEvent?: (
		sessionId: string,
		event: JsonRpcNotification,
	) => void | Promise<void>;
	onPermissionRequest?: (
		sessionId: string,
		request: PermissionRequest,
	) => void | Promise<void>;
};

export function agentOS<TConnParams = undefined>(
	config: AgentOSConfigInput<TConnParams> = {},
): AgentOsActorDefinition<TConnParams> {
	const {
		preview,
		onBeforeConnect,
		onSessionEvent,
		onPermissionRequest,
		...options
	} = config;

	return createAgentOS({
		options,
		preview,
		onBeforeConnect,
		onSessionEvent: onSessionEvent
			? (_ctx, sessionId, event) => onSessionEvent(sessionId, event)
			: undefined,
		onPermissionRequest: onPermissionRequest
			? (_ctx, sessionId, request) => onPermissionRequest(sessionId, request)
			: undefined,
	} as AgentOsActorConfigInput<TConnParams>);
}
