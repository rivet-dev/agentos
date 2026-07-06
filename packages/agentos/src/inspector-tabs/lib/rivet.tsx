// @rivetkit/react integration for the inspector. Builds ONE rivetkit client from
// the iframe's { actorId, authToken } and shares it two ways:
//   - `useActor({ name, id })` → a typed connection to the agent-os actor (used
//     for the live `sessionEvent` stream via `.useEvent`).
//   - `setRivetClient()` → the same client backs the stateless `callAction`
//     transport used by React Query (lib/actor-client.ts).
//
// The client is created once auth is known (it needs the token for the gateway
// URL segment), so this is a provider gated behind the init handshake.
import { createClient, createRivetKitWithClient } from "@rivetkit/react";
import React, { createContext, type ReactNode, useContext, useMemo } from "react";
import { setRivetClient } from "./actor-client";
import { INSPECTOR_ACTOR_NAME, type InspectorRegistry } from "./registry";

// Non-generic factory so `ReturnType<typeof …>` stays clean (no instantiation
// expressions). The `<InspectorRegistry>` on createClient is what types the
// resulting `useActor` connection.
function createRk(authToken: string) {
	const client = createClient<InspectorRegistry>({
		endpoint: window.location.origin,
		token: authToken,
		// Match the gateway's `x-rivet-encoding: json`. The client default is now
		// `bare` (binary), whose decoder yields BigInt for integers and blows up on
		// numeric coercion ("Cannot convert a BigInt value to a number").
		encoding: "json",
		disableMetadataLookup: true,
	});
	return { client, ...createRivetKitWithClient(client) };
}

type RkBundle = ReturnType<typeof createRk>;

const RivetContext = createContext<{ bundle: RkBundle; actorId: string } | null>(null);

export function RivetProvider({
	actorId,
	authToken,
	children,
}: {
	actorId: string;
	authToken: string;
	children: ReactNode;
}) {
	const value = useMemo(() => {
		const bundle = createRk(authToken);
		setRivetClient(bundle.client, actorId);
		return { bundle, actorId };
	}, [actorId, authToken]);
	return <RivetContext.Provider value={value}>{children}</RivetContext.Provider>;
}

/** Typed connection to the agent-os actor, resolved by id. */
export function useAgentOsActor() {
	const ctx = useContext(RivetContext);
	if (!ctx) throw new Error("useAgentOsActor must be used within <RivetProvider>");
	return ctx.bundle.useActor({
		name: INSPECTOR_ACTOR_NAME,
		id: ctx.actorId,
	});
}
