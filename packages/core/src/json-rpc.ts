export interface AcpTimeoutErrorData {
	kind: "acp_timeout";
	method: string;
	id: number | string | null;
	timeoutMs: number;
	exitCode?: number;
	killed?: boolean;
	transportState?: string;
	recentActivity: string[];
}

/**
 * Structured error payload meaning "the session id is not known to the agent" —
 * e.g. a `session/load` / `session/resume` against a session whose on-disk store did
 * not survive a VM teardown. The sidecar normalizes the adapter's native error into
 * this shape so the resume orchestration can distinguish "fall through to a fresh
 * session" from a transport/timeout error (which must propagate). Mirrors the
 * `acp_timeout` convention.
 */
export interface UnknownSessionErrorData {
	kind: "unknown_session";
	/** Optional metadata. The discriminator is `kind` alone — the sidecar's
	 * normalized error carries only `kind`, so this stays optional to keep the
	 * sidecar and client contracts aligned. */
	sessionId?: string;
}

export type JsonRpcErrorData =
	| AcpTimeoutErrorData
	| UnknownSessionErrorData
	| Record<string, unknown>;

export interface JsonRpcRequest {
	jsonrpc: "2.0";
	id: number | string | null;
	method: string;
	params?: unknown;
}

export interface JsonRpcResponse {
	jsonrpc: "2.0";
	id: number | string | null;
	result?: unknown;
	error?: JsonRpcError;
}

export interface JsonRpcError {
	code: number;
	message: string;
	data?: JsonRpcErrorData;
}

export interface JsonRpcNotification {
	jsonrpc: "2.0";
	method: string;
	params?: unknown;
}

export function isAcpTimeoutErrorData(
	value: unknown,
): value is AcpTimeoutErrorData {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return false;
	}
	const record = value as Record<string, unknown>;
	return (
		record.kind === "acp_timeout" &&
		typeof record.method === "string" &&
		(typeof record.id === "number" ||
			typeof record.id === "string" ||
			record.id === null) &&
		typeof record.timeoutMs === "number" &&
		Array.isArray(record.recentActivity) &&
		record.recentActivity.every((entry) => typeof entry === "string")
	);
}

export function isUnknownSessionErrorData(
	value: unknown,
): value is UnknownSessionErrorData {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return false;
	}
	const record = value as Record<string, unknown>;
	return (
		record.kind === "unknown_session" &&
		(record.sessionId === undefined || typeof record.sessionId === "string")
	);
}
