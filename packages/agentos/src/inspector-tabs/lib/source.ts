// Real data source for the inspector tabs — the deliberate replacement for the
// mockup's `createLiveAgentOsSource`. Each query calls a REAL agentOS action via
// the gateway (actor-client) and transforms the result into the display type the
// ported component expects. Action names/shapes are the actual ones, not the
// mockup's aspirational `agentOs*` names.
import { queryOptions } from "@tanstack/react-query";
import { callAction, isInspectorActionError } from "./actor-client";
import type {
	FileContent,
	FsEntry,
	JsonRpcNotification,
	MountInfo,
	PendingPermissionInfo,
	PersistedSessionEvent,
	PersistedSessionRecord,
	ProcessInfo,
	ProcessTreeNode,
	ReaddirEntry,
	RuntimeHealth,
	SignedPreviewUrl,
	SoftwareBundle,
	SoftwareInfo,
	TranscriptEvent,
	VirtualStat,
} from "./types";

const k = (actorId: string, ...rest: string[]) => ["agent-os", actorId, ...rest];

// ── Software ──────────────────────────────────────────────────────────
function softwareInfoToBundle(info: SoftwareInfo): SoftwareBundle {
	const pkg = info.package;
	const scopeIdx = pkg.lastIndexOf("@");
	let name: string;
	if (scopeIdx > 0) name = pkg.slice(scopeIdx).split("/").slice(0, 2).join("/");
	else name = pkg.split("/").filter(Boolean).pop() ?? pkg;
	// Classify off the raw package (which keeps its `@scope/`), not the derived
	// display `name` (which strips the scope for bare scoped packages).
	const source: SoftwareBundle["source"] =
		pkg.startsWith("@rivet-dev/") || pkg.startsWith("@agentos-software/")
			? "rivet-dev"
			: "user";
	return {
		name: `${name} · ${info.kind}`,
		version: info.version ?? "—",
		source,
		binaries: info.commands ?? [],
	};
}

// ── Filesystem helpers ────────────────────────────────────────────────
function joinPath(dir: string, name: string): string {
	return dir === "/" ? `/${name}` : `${dir}/${name}`;
}

export function decodeActionBytes(output: unknown): Uint8Array {
	// rivetkit's json decoder may already hand back a real Uint8Array.
	if (output instanceof Uint8Array) return output;
	// JSON encoding wraps Uint8Array as ["$Uint8Array", base64].
	if (Array.isArray(output) && output[0] === "$Uint8Array" && typeof output[1] === "string") {
		const bin = atob(output[1]);
		const bytes = new Uint8Array(bin.length);
		for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
		return bytes;
	}
	if (Array.isArray(output)) return Uint8Array.from(output as number[]);
	if (typeof output === "string") {
		try {
			const bin = atob(output);
			const bytes = new Uint8Array(bin.length);
			for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
			return bytes;
		} catch {
			return new TextEncoder().encode(output);
		}
	}
	return new Uint8Array();
}

/** Preview limit for the file viewer; larger files load only on request. */
const MAX_PREVIEW_BYTES = 4 * 1024 * 1024;

function bytesToDisplay(bytes: Uint8Array): string | null {
	// Heuristic binary check: NUL byte in the first 8 KiB.
	const probe = bytes.subarray(0, 8192);
	if (probe.includes(0)) return null;
	return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}

// ── Transcript mapper (defensive: unknown ACP updates → "raw") ─────────
// Map a raw JSON-RPC session notification + its ordering seq to a display event.
// Shared by the persisted backfill (`getSessionEvents` → seq from the record)
// and the live stream (`sessionEvent` broadcast → synthetic seq), which both
// carry the same `JsonRpcNotification` shape.
export function mapNotification(n: JsonRpcNotification, seq: number): TranscriptEvent {
	const params = (n?.params ?? {}) as { update?: Record<string, unknown> };
	const u = params.update;
	if (n?.method === "session/update" && u && typeof u === "object") {
		const kind = u.sessionUpdate as string | undefined;
		const text = ((u.content as { text?: string } | undefined)?.text ?? "") as string;
		switch (kind) {
			case "user_message_chunk":
				return { kind: "user", seq, text };
			case "agent_message_chunk":
				return { kind: "assistant", seq, text };
			case "agent_thought_chunk":
				return { kind: "thinking", seq, text };
			case "tool_call":
			case "tool_call_update": {
				// ACP tool content: text blocks become output; diff entries are
				// summarized by path (full diff rendering stays behind "raw").
				const outputParts: string[] = [];
				if (Array.isArray(u.content)) {
					for (const c of u.content as Record<string, unknown>[]) {
						if (!c || typeof c !== "object") continue;
						if (c.type === "content") {
							const inner = c.content as { type?: string; text?: string } | undefined;
							if (inner?.type === "text" && typeof inner.text === "string") {
								outputParts.push(inner.text);
							}
						} else if (c.type === "diff" && typeof c.path === "string") {
							outputParts.push(`[edit] ${c.path}`);
						}
					}
				}
				const locations = Array.isArray(u.locations)
					? (u.locations as { path?: string }[])
							.map((l) => l?.path)
							.filter((p): p is string => typeof p === "string")
					: undefined;
				return {
					kind: "tool",
					seq,
					toolCallId: typeof u.toolCallId === "string" ? u.toolCallId : undefined,
					tool: (u.title as string) ?? (u.toolCallId as string) ?? "tool",
					status: u.status as string | undefined,
					input: u.rawInput,
					output: outputParts.length > 0 ? outputParts.join("\n") : undefined,
					locations: locations && locations.length > 0 ? locations : undefined,
				};
			}
			case "plan": {
				const entries = Array.isArray(u.entries)
					? (u.entries as Record<string, unknown>[]).map((e) => ({
							content:
								typeof e?.content === "string" ? e.content : JSON.stringify(e?.content ?? ""),
							status: typeof e?.status === "string" ? e.status : undefined,
						}))
					: [];
				return { kind: "plan", seq, entries };
			}
			case "current_mode_update":
				return {
					kind: "notice",
					seq,
					text: `Mode changed to ${String(u.currentModeId ?? "unknown")}`,
				};
			case "available_commands_update": {
				const count = Array.isArray(u.availableCommands) ? u.availableCommands.length : 0;
				return {
					kind: "notice",
					seq,
					text: `${count} agent command${count === 1 ? "" : "s"} available`,
				};
			}
			default:
				return { kind: "raw", seq, label: kind ?? n.method, json: u };
		}
	}
	return { kind: "raw", seq, label: n?.method ?? "event", json: n?.params };
}

// Persisted-event adapter: unwrap the record and reuse the notification mapper.
function mapTranscriptEvent(pe: PersistedSessionEvent): TranscriptEvent {
	return mapNotification(pe.event, pe.seq);
}

// ── Query options ─────────────────────────────────────────────────────
export const agentOsSource = {
	softwareQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: k(actorId, "software"),
			queryFn: async () =>
				(await callAction<SoftwareInfo[]>("listSoftware", [])).map(softwareInfoToBundle),
		}),

	processesQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: k(actorId, "processes"),
			queryFn: () => callAction<ProcessInfo[]>("listProcesses", []),
			// Keep the table current while the tab is open; processExit broadcasts
			// also invalidate it immediately.
			refetchInterval: 5_000,
		}),

	// Full kernel process forest (every process, not just SDK-spawned).
	processTreeQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: k(actorId, "process-tree"),
			queryFn: () => callAction<ProcessTreeNode[]>("processTree", [], { timeoutMs: 10_000 }),
			refetchInterval: 5_000,
		}),

	// Lazy per-directory listing via ONE `readdirEntries` call: the sidecar
	// returns every child with its type in a single round-trip (no `readdir` +
	// per-entry `stat`, which wedged the actor on large/virtual dirs). Recursive
	// from root still times out, so the tree fetches one level at a time on expand.
	listDirQueryOptions: (actorId: string, path: string, enabled = true) =>
		queryOptions({
			queryKey: k(actorId, "dir", path),
			enabled,
			// `readdirEntries` returns `null` when `path` is not a listable
			// directory (does not exist / is a file); surface that as `null` so
			// callers can show "not found", distinct from `[]` (empty dir).
			queryFn: async (): Promise<FsEntry[] | null> => {
				const raw = await callAction<ReaddirEntry[] | null>("readdirEntries", [path], {
					timeoutMs: 10_000,
				});
				if (raw === null) return null;
				const entries = raw
					.filter((e) => e.name !== "." && e.name !== "..")
					.map((e): FsEntry => {
						const p = joinPath(path, e.name);
						// Symlinks are reported lstat-style (not followed) → shown as a
						// leaf, like the old per-entry path did. Virtual fs (/proc, …) is
						// flagged so the tree never auto-expands it.
						return {
							name: e.name,
							path: p,
							dir: e.isDirectory,
							symlink: e.isSymbolicLink,
						};
					});
				return entries.sort(
					(a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name),
				);
			},
		}),

	fileContentQueryOptions: (actorId: string, path: string | null, force = false) =>
		queryOptions({
			queryKey: k(actorId, "file", path ?? "", force ? "force" : "guarded"),
			enabled: !!path,
			queryFn: async (): Promise<FileContent> => {
				const p = path as string;
				// Stat first: reading a huge file drags megabytes through the
				// gateway just to preview it. Past the limit, skip the read until
				// the viewer's explicit "Load anyway".
				const stat = await callAction<VirtualStat>("stat", [p]);
				if (!force && stat.size > MAX_PREVIEW_BYTES) {
					return {
						path: p,
						sizeBytes: stat.size,
						mtimeMs: stat.mtimeMs,
						text: null,
						bytes: null,
						oversize: true,
					};
				}
				const bytes = decodeActionBytes(
					await callAction("readFile", [p], { timeoutMs: 30_000 }),
				);
				return {
					path: p,
					sizeBytes: stat.size,
					mtimeMs: stat.mtimeMs,
					text: bytesToDisplay(bytes),
					bytes,
					oversize: false,
				};
			},
		}),

	mountsQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: k(actorId, "mounts"),
			queryFn: () => callAction<MountInfo[]>("listMounts", []),
		}),

	sessionsQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: k(actorId, "sessions"),
			queryFn: () => callAction<PersistedSessionRecord[]>("listPersistedSessions", []),
			// Poll so newly created sessions and running/idle status dots stay current.
			refetchInterval: 10_000,
		}),

	// One-off persisted backfill. Live events after this snapshot arrive via the
	// `sessionEvent` websocket stream (see `useLiveSessionEvents`), so this no
	// longer polls.
	transcriptQueryOptions: (actorId: string, sessionId: string | null) =>
		queryOptions({
			queryKey: k(actorId, "transcript", sessionId ?? ""),
			enabled: !!sessionId,
			queryFn: async () =>
				(await callAction<PersistedSessionEvent[]>("getSessionEvents", [sessionId])).map(
					mapTranscriptEvent,
				),
		}),

	// ── Composer actions (transcript tab) ─────────────────────────────────
	// Imperative (not queries): the composer drives the agent. Streamed output
	// arrives on the existing `sessionEvent` subscription; `sendPrompt` resolves
	// when the turn completes.
	sendPrompt: (sessionId: string, text: string) =>
		callAction<{ text?: string }>("sendPrompt", [sessionId, text]),
	// Older runtimes return the sessionId string; the rivetkit wrapper returns
	// `{ sessionId }` — callers normalize.
	createSession: (agentType: string, options: { env?: Record<string, string> }) =>
		callAction<string | { sessionId: string }>("createSession", [agentType, options]),

	// ── Permission approvals (global banner, permission-prompts.tsx) ─────
	// Answers a pending `permissionRequest` broadcast. The runtime auto-rejects
	// after its permission timeout and another viewer may answer first, so a
	// late reply fails with a typed runtime error ("already answered or
	// expired") — callers render that as already-handled, not a failure.
	respondPermission: (sessionId: string, permissionId: string, reply: "once" | "always" | "reject") =>
		callAction("respondPermission", [sessionId, permissionId, reply]),

	// ── Process control (processes tab) ───────────────────────────────────
	killProcess: (pid: number) => callAction("killProcess", [pid]),
	stopProcess: (pid: number) => callAction("stopProcess", [pid]),

	// ── Shell / PTY (terminal tab) ─────────────────────────────────────────
	// Output arrives on the `shellData`/`shellStderr`/`shellExit` broadcasts.
	openShell: (options: { cols?: number; rows?: number; command?: string; cwd?: string }) =>
		callAction<{ shellId: string }>("openShell", [options]),
	writeShell: (shellId: string, data: string) => callAction("writeShell", [shellId, data]),
	resizeShell: (shellId: string, cols: number, rows: number) =>
		callAction("resizeShell", [shellId, cols, rows]),
	closeShell: (shellId: string) => callAction("closeShell", [shellId]),

	// ── Filesystem mutations (filesystem tab) ──────────────────────────────
	writeFile: (path: string, content: Uint8Array | string) =>
		callAction("writeFile", [path, content], { timeoutMs: 30_000 }),
	mkdir: (path: string) => callAction("mkdir", [path]),
	moveEntry: (from: string, to: string) => callAction("move", [from, to]),
	deleteFile: (path: string, options: { recursive?: boolean }) =>
		callAction("deleteFile", [path, options]),

	// ── Session management (transcript tab) ────────────────────────────────
	closeSession: (sessionId: string) => callAction("closeSession", [sessionId]),

	// ── Signed preview URLs (system tab) ───────────────────────────────────
	createSignedPreviewUrl: (port: number, ttlSeconds: number) =>
		callAction<SignedPreviewUrl>("createSignedPreviewUrl", [port, ttlSeconds]),
	expireSignedPreviewUrl: (token: string) => callAction("expireSignedPreviewUrl", [token]),
};

// ── Runtime health (observe-only actions) ─────────────────────────────
// getRuntimeHealth / listSessions / cancelPrompt are real contract actions on
// the current runtime, dispatched on its non-waking observe-only lane (they
// never boot a sleeping VM). Feature detection stays: vendored tab bundles run
// against OLDER runtimes without these actions, which reject at the contract
// layer (see lib/health.ts's isMissingHealthAction) — the status strip hides
// itself and the composer disables its Stop button.

export const healthQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: k(actorId, "runtime-health"),
		queryFn: () => callAction<RuntimeHealth>("getRuntimeHealth", [], { timeoutMs: 8_000 }),
		refetchInterval: 5_000,
		// Contract-missing is permanent for this actor: never retry, and stop
		// polling (react-query keeps refetching errored queries otherwise).
		retry: false,
		refetchOnMount: false,
	});

/** Live (loaded-in-VM) sessions — used to mark sidebar rows as running. Rows
 * carry the EXTERNAL session id, so they cross-reference listPersistedSessions
 * records directly. Returns null when the runtime doesn't expose the action
 * (callers fall back to record.status). */
export const liveSessionsQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: k(actorId, "live-sessions"),
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

/** Pending permission requests buffered runtime-side — the one-off backfill
 * for the permission banner (permission-prompts.tsx), so requests broadcast
 * while no inspector iframe was open still render. No refetchInterval: after
 * the mount fetch, updates arrive on the `permissionRequest` /
 * `permissionResolved` broadcasts. Returns null when the runtime doesn't
 * expose the action (older runtime) — the banner then stays live-only. */
export const pendingPermissionsQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: k(actorId, "pending-permissions"),
		queryFn: async (): Promise<PendingPermissionInfo[] | null> => {
			try {
				return await callAction<PendingPermissionInfo[]>("listPendingPermissions", []);
			} catch (error) {
				if (isInspectorActionError(error) && error.layer === "contract") return null;
				throw error;
			}
		},
		retry: false,
	});

/** Best-effort prompt cancellation; resolves false when unsupported. A booted
 * runtime with nothing running rejects with a runtime error ("VM is not
 * booted" / unknown session), which propagates to the caller. */
export async function cancelPrompt(sessionId: string): Promise<boolean> {
	try {
		await callAction("cancelPrompt", [sessionId]);
		return true;
	} catch (error) {
		if (isInspectorActionError(error) && error.layer === "contract") return false;
		throw error;
	}
}
