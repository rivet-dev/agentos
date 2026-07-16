/**
 * `@rivet-dev/agentos/client` — Agent OS client surface.
 *
 * Re-exports RivetKit's client entry (`ActorHandle`, `ActorConn`, etc.) and
 * narrows actor creation input from the registered actor definition. This is
 * deliberately kept OFF the root module so that
 * server/Node actor code that imports `@rivet-dev/agentos` never transitively
 * pulls in the browser/client bundle.
 *
 * The browser export condition is preserved through to `rivetkit/client`
 * (which is external here), so consumers resolving this subpath in a browser
 * environment still get RivetKit's browser client build.
 */

import type { Registry } from "rivetkit";
import {
	type ActorAccessor,
	type ActorDefinition,
	type ActorHandle,
	type AnyActorDefinition,
	type Client,
	type ClientConfigInput,
	createClient as createRivetClient,
	type ExtractActorsFromRegistry,
	type GetOptions,
	type CreateOptions as RivetCreateOptions,
} from "rivetkit/client";

export * from "rivetkit/client";

type ActorInput<AD extends AnyActorDefinition> =
	AD extends ActorDefinition<
		any,
		any,
		any,
		any,
		infer TInput,
		any,
		any,
		any,
		any
	>
		? TInput
		: unknown;

export type CreateOptions<AD extends AnyActorDefinition = AnyActorDefinition> =
	Omit<RivetCreateOptions, "input"> & { input?: ActorInput<AD> };

export type GetOrCreateOptions<
	AD extends AnyActorDefinition = AnyActorDefinition,
> = GetOptions & {
	createInRegion?: string;
	createWithInput?: ActorInput<AD>;
};

export type AgentOsActorAccessor<AD extends AnyActorDefinition> = Omit<
	ActorAccessor<AD>,
	"create" | "getOrCreate"
> & {
	create(
		key?: string | string[],
		opts?: CreateOptions<AD>,
	): Promise<ActorHandle<AD>>;
	getOrCreate(
		key?: string | string[],
		opts?: GetOrCreateOptions<AD>,
	): ActorHandle<AD>;
};

export type AgentOsClient<A extends Registry<any>> = Omit<
	Client<A>,
	keyof ExtractActorsFromRegistry<A>
> & {
	[K in keyof ExtractActorsFromRegistry<A>]: AgentOsActorAccessor<
		ExtractActorsFromRegistry<A>[K]
	>;
};

export function createClient<A extends Registry<any>>(
	endpointOrConfig?: string | ClientConfigInput,
): AgentOsClient<A> {
	return createRivetClient<A>(endpointOrConfig) as AgentOsClient<A>;
}
