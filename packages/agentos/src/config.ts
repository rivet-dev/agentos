import type {
	AgentOsOptions,
	JsonRpcNotification,
	NativeMountConfig,
	PermissionRequest,
} from "@rivet-dev/agentos-core";
import {
	agentOsOptionFieldSchemas,
	nativeMountConfigSchema,
	sharedSidecarConfigSchema,
} from "@rivet-dev/agentos-core";
import type { ActorContext, BeforeConnectContext } from "rivetkit";
import { z } from "zod/v4";
import type { AgentOsActorState, AgentOsActorVars } from "./types.js";

const zFunction = <
	T extends (...args: any[]) => any = (...args: unknown[]) => unknown,
>() => z.custom<T>((val) => typeof val === "function");

export const nativeAgentOsOptionsSchema = z
	// Native actor options are a JSON-serializable subset of AgentOsOptions.
	// Keep this allow-list in sync with buildConfigJson() and
	// crates/agentos-actor-plugin/src/config.rs::AgentOsConfigJson.
	.object({
		software: agentOsOptionFieldSchemas.software,
		defaultSoftware: agentOsOptionFieldSchemas.defaultSoftware,
		loopbackExemptPorts: agentOsOptionFieldSchemas.loopbackExemptPorts,
		allowedNodeBuiltins: agentOsOptionFieldSchemas.allowedNodeBuiltins,
		rootFilesystem: agentOsOptionFieldSchemas.rootFilesystem,
		mounts: z.array(nativeMountConfigSchema).optional(),
		additionalInstructions: agentOsOptionFieldSchemas.additionalInstructions,
		permissions: agentOsOptionFieldSchemas.permissions,
		sidecar: sharedSidecarConfigSchema.optional(),
		limits: agentOsOptionFieldSchemas.limits,
	})
	.strict();

/**
 * RivetKit actor lifecycle/transport options forwarded to `actor({ options })`.
 *
 * Validated downstream by RivetKit's own actor config schema, so this stays a
 * permissive pass-through allow-list. `agentOS()` overlays the never-hit
 * defaults in `DEFAULT_AGENTOS_ACTOR_OPTIONS` (see actor.ts) and lets any value
 * here win, so callers can still tighten a bound when they want one.
 */
export const agentOsActorOptionsSchema = z
	.record(z.string(), z.unknown())
	.optional();

export const agentOsActorConfigSchema = z
	.object({
		options: nativeAgentOsOptionsSchema.optional(),
		actorOptions: agentOsActorOptionsSchema,
		preview: z
			.object({
				defaultExpiresInSeconds: z.number().positive().default(3600),
				maxExpiresInSeconds: z.number().positive().default(86400),
			})
			.strict()
			.prefault(() => ({})),
		onBeforeConnect: zFunction().optional(),
		onSessionEvent: zFunction().optional(),
		onPermissionRequest: zFunction().optional(),
	})
	.strict();

// --- Typed config types (generic callbacks overlaid on the Zod schema) ---

/**
 * Type mirror of `nativeAgentOsOptionsSchema`.
 *
 * Keep this in sync with the schema above and the Rust serde mirror at
 * `crates/agentos-actor-plugin/src/config.rs::AgentOsConfigJson`.
 */
export type NativeAgentOsOptions = Pick<
	AgentOsOptions,
	| "software"
	| "defaultSoftware"
	| "loopbackExemptPorts"
	| "allowedNodeBuiltins"
	| "rootFilesystem"
	| "additionalInstructions"
	| "permissions"
	| "limits"
> & {
	mounts?: NativeMountConfig[];
	sidecar?: { kind: "shared"; pool?: string };
};

type AgentOsActorContext<TConnParams> = ActorContext<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	any
>;

interface AgentOsActorConfigCallbacks<TConnParams> {
	onBeforeConnect?: (
		c: BeforeConnectContext<
			AgentOsActorState,
			AgentOsActorVars,
			undefined,
			any
		>,
		params: TConnParams,
	) => void | Promise<void>;
	onSessionEvent?: (
		c: AgentOsActorContext<TConnParams>,
		sessionId: string,
		event: JsonRpcNotification,
	) => void | Promise<void>;
	onPermissionRequest?: (
		c: AgentOsActorContext<TConnParams>,
		sessionId: string,
		request: PermissionRequest,
	) => void | Promise<void>;
}

// Parsed config (after Zod defaults/transforms applied).
export type AgentOsActorConfig<TConnParams = undefined> = Omit<
	z.infer<typeof agentOsActorConfigSchema>,
	"options" | "onBeforeConnect" | "onSessionEvent" | "onPermissionRequest"
> &
	{ options?: NativeAgentOsOptions } &
	AgentOsActorConfigCallbacks<TConnParams>;

// Input config (what users pass in before Zod transforms).
export type AgentOsActorConfigInput<TConnParams = undefined> = Omit<
	z.input<typeof agentOsActorConfigSchema>,
	"options" | "onBeforeConnect" | "onSessionEvent" | "onPermissionRequest"
> &
	{ options?: NativeAgentOsOptions } &
	AgentOsActorConfigCallbacks<TConnParams>;
