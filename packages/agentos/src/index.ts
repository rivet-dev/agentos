// Rust-backed agent-os actor surface (native actor plugin / cdylib).
//
// Only the `agentOs()` definition function, the config schema, the
// `nodeModulesMount` helper, the plugin-path resolver, and the public domain
// types are exported. All actor lifecycle + action dispatch live in the Rust
// plugin (`crates/agentos-actor-plugin`), loaded by RivetKit via the generic
// native-plugin ABI.

export {
	agentOs,
	type AgentOsActorDefinition,
	buildConfigJson,
	nodeModulesMount,
	type NodeModulesMountConfig,
} from "./actor.js";

export {
	type AgentOsActorConfig,
	type AgentOsActorConfigInput,
	agentOsActorConfigSchema,
} from "./config.js";

export { getPluginPath } from "./plugin-binary.js";

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
