import type {
	AgentOsActorHandle,
	AgentOsClient,
} from "../../generated/contract";

let client: AgentOsClient | undefined;
let handle: AgentOsActorHandle | undefined;
let actorId: string | undefined;

export function setRivetClient(
	nextClient: AgentOsClient,
	nextActorId: string,
): void {
	client = nextClient;
	actorId = nextActorId;
	handle = undefined;
}

export function getAgentOsHandle(): AgentOsActorHandle {
	if (!client || !actorId) {
		throw new Error("agentOS inspector client has not been initialized");
	}
	handle ??= client.getForId("agentOS", actorId);
	return handle as AgentOsActorHandle;
}

export function tabIdFromUrl(): string | undefined {
	const parts = window.location.pathname.split("/").filter(Boolean);
	const index = parts.indexOf("custom-tabs");
	return index >= 0 ? parts[index + 1] : undefined;
}

export type ActionErrorLayer =
	| "gateway"
	| "auth"
	| "contract"
	| "runtime"
	| "timeout";

export class InspectorActionError extends Error {
	constructor(
		readonly layer: ActionErrorLayer,
		readonly action: string,
		message: string,
		readonly hint: string,
	) {
		super(message);
		this.name = "InspectorActionError";
	}
}

export function isInspectorActionError(
	error: unknown,
): error is InspectorActionError {
	return (
		error instanceof InspectorActionError ||
		(typeof error === "object" &&
			error !== null &&
			(error as { name?: string }).name === "InspectorActionError")
	);
}

function classifyActionError(
	action: string,
	error: unknown,
): InspectorActionError {
	const message = error instanceof Error ? error.message : String(error);
	const rivet = (error ?? {}) as {
		code?: string;
		statusCode?: number;
		name?: string;
	};
	if (rivet.name === "AbortError" || /\baborted\b/i.test(message)) {
		return new InspectorActionError(
			"timeout",
			action,
			`${action} timed out`,
			"The VM may still be booting. Retry in a few seconds.",
		);
	}
	if (/was not found/i.test(message) && /action/i.test(message)) {
		return new InspectorActionError(
			"contract",
			action,
			`This actor does not expose ${action}.`,
			"The inspector and actor contract versions do not match.",
		);
	}
	if (
		rivet.statusCode === 401 ||
		rivet.statusCode === 403 ||
		rivet.code === "unauthorized" ||
		/unauthorized|forbidden/i.test(message)
	) {
		return new InspectorActionError(
			"auth",
			action,
			message,
			"Reload the inspector to refresh its actor token.",
		);
	}
	if (
		error instanceof TypeError ||
		/fetch failed|failed to fetch/i.test(message)
	) {
		return new InspectorActionError(
			"gateway",
			action,
			message,
			"The actor gateway could not be reached.",
		);
	}
	return new InspectorActionError(
		"runtime",
		action,
		message || `${action} failed`,
		"See the actor logs for the underlying error.",
	);
}

export async function runInspectorAction<T>(
	action: string,
	call: (handle: AgentOsActorHandle) => Promise<T>,
	boundHandle?: AgentOsActorHandle,
): Promise<T> {
	try {
		return await call(boundHandle ?? getAgentOsHandle());
	} catch (error) {
		throw classifyActionError(action, error);
	}
}
