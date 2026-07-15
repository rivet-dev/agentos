// Display + raw types for the inspector tabs. Raw types match the agentOS
// action return shapes; the source adapter (source.ts) transforms raw → display.

// ── Software ──────────────────────────────────────────────────────────
export interface SoftwareBundle {
	name: string;
	version: string;
	source: "rivet-dev" | "user";
	binaries: string[]; // command names the package ships (from SoftwareInfo.commands)
}
export interface SoftwareInfo {
	package: string;
	kind: "wasm-commands" | "agent" | "tool";
	version: string | null;
	/** Command names this package ships (wasm-commands only; [] for agent/tool). */
	commands: string[];
}

// ── Processes ─────────────────────────────────────────────────────────
/** Actual `listProcesses` serialized shape (a subset of the kernel struct;
 * ppid/cwd/cpu/mem/signal/stdout are NOT exposed — PARTIAL). Only processes
 * started via the SDK `spawn` are listed; `startedAt` is the spawn time. */
export interface ProcessInfo {
	pid: number;
	command: string;
	args: string[];
	running: boolean;
	exitCode: number | null;
	/** Epoch milliseconds when the process was spawned. */
	startedAt: number;
}
/** Raw `allProcesses`/`processTree` node fields — the full kernel process
 * table (every process, not just SDK-spawned). Mirrors the Rust wire shape;
 * `startTime`/`exitTime` are epoch milliseconds. */
export interface KernelProcessInfo {
	pid: number;
	ppid: number;
	pgid: number;
	sid: number;
	driver: string;
	command: string;
	args: string[];
	cwd: string;
	status: "running" | "exited";
	exitCode: number | null;
	startTime: number;
	exitTime: number | null;
}
/** Raw `processTree` node: kernel info + children. */
export interface ProcessTreeNode extends KernelProcessInfo {
	children: ProcessTreeNode[];
}
/** Live `processOutput` broadcast payload mirror (Rust owns broadcasts).
 * `data` arrives Uint8Array-shaped but encoding-dependent — normalize with
 * `decodeActionBytes`. Only SDK-`spawn`ed pids have output pumps. */
export interface ProcessOutputPayload {
	pid: number;
	stream: "stdout" | "stderr";
	data: unknown;
}
/** Live `processExit` broadcast payload mirror. */
export interface ProcessExitPayload {
	pid: number;
	exitCode: number;
}

// ── Shell / terminal ──────────────────────────────────────────────────
/** Live `shellData`/`shellStderr` broadcast payload mirror. `data` is
 * Uint8Array-shaped but encoding-dependent — normalize with
 * `decodeActionBytes`. */
export interface ShellDataPayload {
	shellId: string;
	data: unknown;
}
/** Live `shellExit` broadcast payload mirror. */
export interface ShellExitPayload {
	shellId: string;
	exitCode: number;
}

/** Mirror of the `agentCrashed` broadcast payload (Rust owns broadcasts). */
export interface AgentCrashedPayload {
	sessionId: string;
	event: {
		agentType?: string;
		exitCode?: number | null;
		restart?: string;
		restartCount?: number;
	};
}

/** Raw `createSignedPreviewUrl` result. `path` is relative to the gateway
 * origin serving this iframe. */
export interface SignedPreviewUrl {
	path: string;
	token: string;
	port: number;
	expiresAt: number;
}

// ── Filesystem ────────────────────────────────────────────────────────
/** Raw `readdirRecursive` entry. */
export interface DirEntry {
	path: string;
	type: "file" | "directory" | "symlink";
	size: number;
}
/** One directory entry, fetched lazily per-level (readdir + stat). Recursive
 * `readdirRecursive("/")` times out on a real VM fs, so the tree loads on
 * demand: each expanded dir fetches just its own children. */
export interface FsEntry {
	name: string;
	path: string;
	dir: boolean;
	size?: number;
	/** Reported lstat-style (not followed); rendered with a link marker. */
	symlink?: boolean;
	/** Virtual/system fs (/proc, /sys, …) — shown but not stat-ed or expanded,
	 * because touching it wedges the VM sidecar. */
	virtual?: boolean;
}
/** Raw `readdirEntries` entry — one typed child in a single round-trip. No
 * `size` (the fast path skips the per-entry `stat`); the file viewer stats on open. */
export interface ReaddirEntry {
	name: string;
	isDirectory: boolean;
	isSymbolicLink: boolean;
}
/** Raw `stat` shape (subset we use). */
export interface VirtualStat {
	size: number;
	mtimeMs: number;
	isDirectory: boolean;
	isSymbolicLink: boolean;
}
export interface FileContent {
	path: string;
	sizeBytes: number;
	mtimeMs: number;
	text: string | null; // null = binary or not loaded
	/** Raw bytes for download / image preview; null when skipped (oversize). */
	bytes: Uint8Array | null;
	/** True when the file exceeded the preview limit and was not read. */
	oversize: boolean;
}

// ── Mounts ────────────────────────────────────────────────────────────
/** Raw `listMounts` entry — echoes the actor's declarative mount config
 * (`MountInfoDto`). The kernel has no runtime mount table to enumerate. */
export interface MountInfo {
	path: string;
	kind: string;
	readOnly: boolean;
	config?: unknown | null;
}

// ── Sessions / transcript ─────────────────────────────────────────────
export interface PersistedSessionRecord {
	sessionId: string;
	agentType: string;
	createdAt: number;
	/** VM-liveness activity status: "running" = loaded in the VM, "idle" =
	 * persisted but hibernated (resumable). Absent on runtimes whose records
	 * carry no status — liveness then comes from `listSessions` (lib/source.ts). */
	status?: "running" | "idle";
}
export interface JsonRpcNotification {
	jsonrpc: "2.0";
	method: string;
	params?: unknown;
}
/** Live `sessionEvent` broadcast payload (the websocket stream). Unlike the
 * persisted record it carries no `seq`/`createdAt`. */
export interface SessionEventPayload {
	sessionId: string;
	event: JsonRpcNotification;
}
/** Raw `getSessionEvents` row — the full persisted event, not just the bare
 * notification, so we keep `seq` (ordering/keys) and `createdAt` (timestamps). */
export interface PersistedSessionEvent {
	sessionId: string;
	seq: number;
	event: JsonRpcNotification;
	createdAt: number;
}
/** Mapped, displayable transcript event (defensive; unknown → "raw"). Carries
 * the source `seq` for stable keys/ordering. Tool events keep `toolCallId` so
 * the render pipeline can merge a call and its status updates into one card. */
export type TranscriptEvent = { seq: number } & (
	| { kind: "user" | "assistant" | "thinking"; text: string }
	| {
			kind: "tool";
			tool: string;
			toolCallId?: string;
			status?: string;
			input?: unknown;
			output?: string;
			locations?: string[];
	  }
	| { kind: "plan"; entries: { content: string; status?: string }[] }
	| { kind: "notice"; text: string }
	| { kind: "permission"; text: string }
	| { kind: "raw"; label: string; json: unknown }
	| { kind: "error"; text: string }
);

/** Mirror of the `permissionRequest` broadcast payload (Rust owns broadcasts).
 * The agent's turn blocks on the reply and the runtime auto-rejects after its
 * permission timeout (~120s). */
export interface PermissionRequestPayload {
	sessionId: string;
	request: {
		permissionId: string;
		description?: string;
		params: Record<string, unknown>;
	};
}
/** Mirror of the `permissionResolved` broadcast payload (Rust owns
 * broadcasts): a `respondPermission` succeeded, so any other viewer's pending
 * card for this `sessionId:permissionId` is stale and should drop. */
export interface PermissionResolvedPayload {
	sessionId: string;
	permissionId: string;
	reply: "once" | "always" | "reject";
}
/** Raw `listPendingPermissions` row (observe-only; never boots the VM) — an
 * unanswered permission request buffered runtime-side so a banner opened
 * AFTER the `permissionRequest` broadcast can still backfill it. Same
 * `sessionId:permissionId` identity as the broadcast; `requestedAt` is the
 * runtime's receipt time (epoch ms), used for the ~120s expiry countdown. */
export interface PendingPermissionInfo {
	sessionId: string;
	permissionId: string;
	description?: string;
	params: Record<string, unknown>;
	requestedAt: number;
}

// ── Runtime health (`getRuntimeHealth`, observe-only; see lib/source.ts) ──
export interface RuntimeLimitWarning {
	ts: number;
	limit: string;
	category: string;
	observed: number;
	capacity: number;
	fillPercent: number;
}
export interface RuntimeAgentExit {
	ts: number;
	sessionId: string;
	agentType: string;
	exitCode: number | null;
	restart: string;
	restartCount: number;
}
export interface RuntimeHealth {
	booted: boolean;
	sessions: number | null;
	sidecar: { state: string; activeVmCount: number } | null;
	warnings: RuntimeLimitWarning[];
	agentExits: RuntimeAgentExit[];
	stderrTail: { ts: number; line: string }[];
}

/** Live `vmShutdown` broadcast payload mirror (Rust owns broadcasts). */
export interface VmShutdownPayload {
	reason?: "sleep" | "destroy" | "error" | string;
}
