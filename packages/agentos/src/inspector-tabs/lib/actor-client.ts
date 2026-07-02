// Same-origin agentOS actor client for inspector-tab iframes, built on the
// rivetkit browser client (+ @rivetkit/react's re-exported `createClient`).
//
// The dashboard serves each custom tab at `/gateway/<actorId>@<authToken>/
// inspector/custom-tabs/<id>/` and posts an `init` message with the actorId +
// authToken. We attach to the PRE-PROVISIONED actor by id via
// `client.getForId(name, actorId)`. For a `getForId` handle rivetkit resolves
// the gateway target directly to `/gateway/<actorId>@<authToken>/...` with NO
// engine/manager metadata lookup (that only happens for key-based queries), so
// both actions AND the live-event websocket work with only the creds the iframe
// already has. `disableMetadataLookup` also suppresses the client's startup
// `/metadata` probe, which the actor-scoped gateway origin does not serve.
//
// The actor `name` below is a label only: getForId's direct path never uses it
// (the name is checked solely against an engine lookup we disable), so any
// string is fine.
import { createClient } from "@rivetkit/react";
import type { SessionEventPayload } from "./types";
import { isMockMode, mockCallAction } from "./mock";

const ACTOR_NAME = "agentos";

interface Auth {
	actorId: string;
	authToken: string;
}

// Loose handle/conn typing: the inspector does not import the server registry,
// so the actor is untyped here (string action names, `.on` events).
// biome-ignore lint/suspicious/noExplicitAny: untyped cross-origin actor client
type AnyClient = any;
// biome-ignore lint/suspicious/noExplicitAny: untyped actor handle
type AnyHandle = any;
// biome-ignore lint/suspicious/noExplicitAny: untyped actor connection
type AnyConn = any;

let auth: Auth | undefined;
let client: AnyClient | undefined;
let handle: AnyHandle | undefined;
let conn: AnyConn | undefined;

export function setAuth(next: Auth): void {
	auth = next;
}

export function getAuth(): Auth | undefined {
	return auth;
}

/** The tab id this iframe is rendering, parsed from its own gateway URL. */
export function tabIdFromUrl(): string | undefined {
	const parts = window.location.pathname.split("/").filter(Boolean);
	const idx = parts.indexOf("custom-tabs");
	return idx >= 0 ? parts[idx + 1] : undefined;
}

export interface ActionError {
	group?: string;
	code?: string;
	message?: string;
}

/** Lazily construct the rivetkit client + actor handle from the iframe creds. */
function getHandle(): AnyHandle {
	if (!auth) throw new Error("not initialized: missing actorId/authToken");
	if (!client) {
		client = createClient({
			// Same-origin: the dashboard/gateway that served this iframe.
			endpoint: window.location.origin,
			// Embedded into the `@<token>` gateway URL segment by rivetkit.
			token: auth.authToken,
			encoding: "json",
			// Skip the startup `/metadata` probe (engine API, not reachable here).
			disableMetadataLookup: true,
		});
	}
	if (!handle) handle = client.getForId(ACTOR_NAME, auth.actorId);
	return handle;
}

/** The persistent actor connection (opened on first use) for live events. */
function getConnection(): AnyConn {
	if (!conn) conn = getHandle().connect();
	return conn;
}

/** Invoke an actor action over the same-origin gateway.
 *
 * Backed by rivetkit's `handle.action`, which POSTs to
 * `/gateway/<actorId>@<authToken>/action/<name>` with `x-rivet-encoding: json`
 * — the exact wire format the previous hand-rolled fetch client replicated.
 *
 * `timeoutMs` aborts the request (via AbortController) on expiry — important
 * because some VM actions (e.g. `stat` on /proc, /sys) hang server-side, and an
 * un-aborted hung request keeps a browser connection slot open. Enough of those
 * exhaust the ~6-connection pool and starve every other request.
 */
export async function callAction<T = unknown>(
	name: string,
	args: unknown[] = [],
	opts: { timeoutMs?: number } = {},
): Promise<T> {
	// Dummy/offline mode: serve every action from in-memory fixtures, no actor.
	if (isMockMode()) return mockCallAction<T>(name, args);
	const h = getHandle();
	const ctrl = new AbortController();
	const timer = opts.timeoutMs ? setTimeout(() => ctrl.abort(), opts.timeoutMs) : undefined;
	try {
		return (await h.action({ name, args, signal: ctrl.signal })) as T;
	} finally {
		if (timer) clearTimeout(timer);
	}
}

/** Subscribe to the actor's live `sessionEvent` broadcasts (JSON-RPC session
 * notifications). Returns an unsubscribe function. No-ops in mock mode (there
 * is no live actor; the transcript still renders its persisted backfill). */
export function subscribeSessionEvents(cb: (payload: SessionEventPayload) => void): () => void {
	if (isMockMode()) return () => {};
	const c = getConnection();
	return c.on("sessionEvent", (payload: SessionEventPayload) => cb(payload));
}
