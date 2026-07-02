// Type-only registry so @rivetkit/react's `useActor` is typed against the
// agent-os actor definition. The inspector never imports the server registry at
// runtime — it attaches to a pre-provisioned actor by id — so this exists purely
// to give `useActor({ name, id })` typed action methods on the connection.
//
// The name is arbitrary: `getForId`'s direct-gateway path never sends it on the
// wire (it's only checked against an engine lookup we disable). It just has to
// match the key used in the registry type below.
import type { Registry } from "rivetkit";
import type { AgentOsActorDefinition } from "../../actor";

export const INSPECTOR_ACTOR_NAME = "agent-os-internal";

export type InspectorRegistry = Registry<
	Record<typeof INSPECTOR_ACTOR_NAME, AgentOsActorDefinition<undefined>>
>;
