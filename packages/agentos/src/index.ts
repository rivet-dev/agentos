// Rust-backed agent-os actor surface (native actor plugin / cdylib).
//
// Only the `agentOs()` definition function, the config schema, the
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
	agentOs as createAgentOs,
} from "./actor.js";
import type {
	AgentOsActorConfigInput,
	NativeAgentOsOptions,
} from "./config.js";

export { setup } from "rivetkit";

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
export { createAgentOs as agentOs };

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

	return createAgentOs({
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
