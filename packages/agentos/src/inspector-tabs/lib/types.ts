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
	text: string | null; // null = binary
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
	 * persisted but hibernated (resumable). */
	status: "running" | "idle";
}
export interface JsonRpcNotification {
	jsonrpc: "2.0";
	method: string;
	params?: unknown;
}
/** Live `sessionEvent` broadcast payload (the websocket stream). Unlike the
 * persisted record it carries no `seq`/`createdAt`. `sessionId` is optional
 * because the broadcast does not always include it. */
export interface SessionEventPayload {
	sessionId?: string;
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
 * the source `seq` for stable keys/ordering. */
export type TranscriptEvent = { seq: number } & (
	| { kind: "user" | "assistant" | "thinking"; text: string }
	| { kind: "tool"; tool: string; status?: string }
	| { kind: "raw"; label: string; json: unknown }
);
