import { createRivetKit } from "@rivetkit/react";

// The RivetKit server (server.ts) listens here by default.
const ENDPOINT =
	(import.meta.env.VITE_AGENTOS_ENDPOINT as string | undefined) ??
	"http://localhost:6642";

// Untyped registry: the actor's action/event surface is exercised by name at
// runtime, which keeps the browser bundle free of any server-only imports.
export const { useActor } = createRivetKit<any>(ENDPOINT);

/** Name of the actor defined in `setup({ use: { shellVm } })`. */
export const ACTOR_NAME = "shellVm";
