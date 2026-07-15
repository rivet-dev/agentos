// OPTIONAL actor actions, deliberately outside lib/source.ts: the actor.test.ts
// audit requires every source.ts action to exist in the generated contract, but
// these two are runtime extensions that some hosts provide and some don't —
// `getRuntimeHealth` is a host-side shim (e.g. rivet-opencode-example) and
// `cancelPrompt` ships in the rivetkit 2.3.3 agent-os wrapper but not the
// generated contract here. Callers must feature-detect: both throw a
// contract-layer InspectorActionError when the actor lacks them (the status
// strip hides itself; the composer disables its Stop button).
import { queryOptions } from "@tanstack/react-query";
import { callAction, isInspectorActionError } from "./actor-client";
import type { RuntimeHealth } from "./types";

export const healthQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: ["agent-os", actorId, "runtime-health"],
		queryFn: () => callAction<RuntimeHealth>("getRuntimeHealth", [], { timeoutMs: 8_000 }),
		refetchInterval: 5_000,
		// Contract-missing is permanent for this actor: never retry, and stop
		// polling (react-query keeps refetching errored queries otherwise).
		retry: false,
		refetchOnMount: false,
	});

export function isMissingHealthAction(error: unknown): boolean {
	return isInspectorActionError(error) && error.layer === "contract";
}

/** Live (loaded-in-VM) sessions — used to mark sidebar rows as running. The
 * current rivetkit wrapper's listPersistedSessions rows carry no status field,
 * so liveness comes from cross-referencing listSessions. Returns null when the
 * runtime doesn't expose the action (callers fall back to record.status). */
export const liveSessionsQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: ["agent-os", actorId, "live-sessions"],
		queryFn: async (): Promise<Set<string> | null> => {
			try {
				const live = await callAction<{ sessionId: string }[]>("listSessions", []);
				return new Set(live.map((s) => s.sessionId));
			} catch (error) {
				if (isInspectorActionError(error) && error.layer === "contract") return null;
				throw error;
			}
		},
		refetchInterval: 10_000,
		retry: false,
	});

/** Best-effort prompt cancellation; resolves false when unsupported. */
export async function cancelPrompt(sessionId: string): Promise<boolean> {
	try {
		await callAction("cancelPrompt", [sessionId]);
		return true;
	} catch (error) {
		if (isInspectorActionError(error) && error.layer === "contract") return false;
		throw error;
	}
}
