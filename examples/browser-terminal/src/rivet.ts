import { createRivetKit } from "@rivetkit/react";

const ENDPOINT =
	(import.meta.env.VITE_AGENTOS_ENDPOINT as string | undefined) ??
	"http://localhost:6420";

export const { useActor } = createRivetKit<any>(ENDPOINT);

export const ACTOR_NAME = "shellVm";
