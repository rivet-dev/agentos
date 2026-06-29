/**
 * Typed action surface for the agentOS VM actor.
 *
 * ⚠️ SOURCE OF TRUTH / KEEP IN SYNC ⚠️
 * The actual dispatch is implemented in Rust at
 *   crates/agentos-actor-plugin/src/actions/mod.rs  (fn `dispatch`)
 * This interface MUST mirror that match statement one-to-one: every `"name" =>`
 * arm there needs a corresponding method here with matching positional args and
 * the serialized return type. When you add/rename/retype an action in
 * `mod.rs`, update this interface in the same change (and vice-versa).
 *
 * RivetKit turns each `(ctx, ...args) => Promise<R>` entry into a client handle
 * method `(...args) => Promise<R>` (it strips the leading context arg). Wiring
 * this as the actions type param of `AgentOsActorDefinition` is what makes
 * `createClient<typeof registry>()` return a fully-typed handle instead of the
 * old `any`/`unknown` surface.
 */
import type {
	ExecResult,
	ProcessInfo,
	VirtualStat,
} from "@rivet-dev/agentos-core";
import type {
	PersistedSessionEvent,
	PersistedSessionRecord,
	PromptResult,
	SerializableCronJobInfo,
	SerializableCronJobOptions,
} from "./types.js";

/** The leading actor context arg; stripped from the client-facing method. */
// biome-ignore lint/suspicious/noExplicitAny: ctx is server-side only and never reaches the typed client surface.
type Ctx = any;

/** Directory entry returned by `readdir` / `readdirRecursive`. */
export interface DirEntry {
	path: string;
	name: string;
	type: "file" | "directory" | "symlink";
}

/** A process started via `spawn` (mirrors the Rust spawn handle DTO). */
export interface SpawnedProcess {
	pid: number;
}

/** Options accepted by `spawn` (mirrors `SpawnActionOptions`). */
export interface SpawnActionOptions {
	env?: Record<string, string>;
	cwd?: string;
	streamStdin?: boolean;
}

/** Handle returned by `scheduleCron` (mirrors `ScheduledCronDto`). */
export interface ScheduledCronJob {
	id: string;
}

/** Options accepted by `vmFetch` (mirrors the Rust `FetchOptions`). */
export interface VmFetchOptions {
	method?: string;
	headers?: Record<string, string>;
	body?: string | Uint8Array;
}

/** Response from `vmFetch` (mirrors `FetchResponseDto`). */
export interface VmFetchResponse {
	status: number;
	statusText: string;
	headers: Record<string, string>;
	body: Uint8Array;
}

/** Options accepted by `createSession` (mirrors `CreateSessionOptionsDto`). */
export interface CreateSessionOptions {
	cwd?: string;
	env?: Record<string, string>;
	skipOsInstructions?: boolean;
	additionalInstructions?: string;
}

/** Result of `createSignedPreviewUrl` (mirrors `SignedPreviewUrlDto`). */
export interface SignedPreviewUrl {
	url: string;
	token: string;
	expiresAt: number;
}

/** Options accepted by `openShell` (mirrors `OpenShellActionOptions`). */
export interface OpenShellActionOptions {
	command?: string;
	args?: string[];
	env?: Record<string, string>;
	cwd?: string;
	cols?: number;
	rows?: number;
}

/** Handle returned by `openShell` (mirrors `OpenShellDto`). */
export interface OpenShellResult {
	shellId: string;
}

/** Per-entry result of the batch `writeFiles` / `readFiles` actions. */
export interface WriteFileResult {
	path: string;
	ok: boolean;
	error?: string;
}
export interface ReadFileResult {
	path: string;
	content?: Uint8Array;
	error?: string;
}

/** One configured mount, returned by `listMounts`. Mirrors `MountInfoDto`. */
export interface MountInfo {
	path: string;
	/** Native mount plugin id. */
	kind: "host_dir" | "s3" | "google_drive" | "sandbox_agent";
	/** Provider-specific config detail (null when the plugin carries none). */
	config: unknown;
	readOnly: boolean;
}

/** One configured software package, returned by `listSoftware`. Mirrors `SoftwareInfoDto`. */
export interface SoftwareInfo {
	package: string;
	/** Kebab-case `SoftwareKind` tag. */
	kind: "wasm-commands" | "agent" | "tool";
	version: string | null;
}

/**
 * The agentOS VM actor's action map. Keep one method per Rust `dispatch` arm.
 *
 * Declared as a `type` (not `interface`) so it satisfies RivetKit's
 * `Actions<…>` constraint, which expects an implicit string index signature.
 */
export type AgentOsActions = {
	// ── Filesystem ────────────────────────────────────────────────────
	readFile: (c: Ctx, path: string) => Promise<Uint8Array>;
	writeFile: (c: Ctx, path: string, content: string | Uint8Array) => Promise<void>;
	stat: (c: Ctx, path: string) => Promise<VirtualStat>;
	mkdir: (c: Ctx, path: string) => Promise<void>;
	readdir: (c: Ctx, path: string) => Promise<DirEntry[]>;
	exists: (c: Ctx, path: string) => Promise<boolean>;
	move: (c: Ctx, from: string, to: string) => Promise<void>;
	deleteFile: (c: Ctx, path: string, options?: { recursive?: boolean }) => Promise<void>;
	writeFiles: (
		c: Ctx,
		entries: { path: string; content: string | Uint8Array }[],
	) => Promise<WriteFileResult[]>;
	readFiles: (c: Ctx, paths: string[]) => Promise<ReadFileResult[]>;
	readdirRecursive: (c: Ctx, path: string) => Promise<DirEntry[]>;

	// ── Processes ─────────────────────────────────────────────────────
	exec: (c: Ctx, command: string) => Promise<ExecResult>;
	// Output streams to connected clients as `processOutput` events; the exit
	// code also broadcasts as `processExit`.
	spawn: (
		c: Ctx,
		command: string,
		args: string[],
		options?: SpawnActionOptions,
	) => Promise<SpawnedProcess>;
	waitProcess: (c: Ctx, pid: number) => Promise<number>;
	killProcess: (c: Ctx, pid: number) => Promise<void>;
	stopProcess: (c: Ctx, pid: number) => Promise<void>;
	listProcesses: (c: Ctx) => Promise<ProcessInfo[]>;
	allProcesses: (c: Ctx) => Promise<ProcessInfo[]>;
	processTree: (c: Ctx) => Promise<ProcessInfo[]>;
	getProcess: (c: Ctx, pid: number) => Promise<ProcessInfo>;
	writeProcessStdin: (c: Ctx, pid: number, data: string | Uint8Array) => Promise<void>;
	closeProcessStdin: (c: Ctx, pid: number) => Promise<void>;

	// ── Shells (PTY) ──────────────────────────────────────────────────
	// Output streams to connected clients as `shellData` / `shellStderr`
	// events; the exit code also broadcasts as `shellExit`.
	openShell: (c: Ctx, options?: OpenShellActionOptions) => Promise<OpenShellResult>;
	writeShell: (c: Ctx, shellId: string, data: string | Uint8Array) => Promise<void>;
	resizeShell: (c: Ctx, shellId: string, cols: number, rows: number) => Promise<void>;
	closeShell: (c: Ctx, shellId: string) => Promise<void>;
	waitShell: (c: Ctx, shellId: string) => Promise<number>;

	// ── Network ───────────────────────────────────────────────────────
	vmFetch: (
		c: Ctx,
		port: number,
		url: string,
		options?: VmFetchOptions,
	) => Promise<VmFetchResponse>;

	// ── Cron ──────────────────────────────────────────────────────────
	scheduleCron: (c: Ctx, options: SerializableCronJobOptions) => Promise<ScheduledCronJob>;
	listCronJobs: (c: Ctx) => Promise<SerializableCronJobInfo[]>;
	cancelCronJob: (c: Ctx, id: string) => Promise<void>;

	// ── Sessions ──────────────────────────────────────────────────────
	createSession: (c: Ctx, agentType: string, options?: CreateSessionOptions) => Promise<string>;
	sendPrompt: (c: Ctx, sessionId: string, text: string) => Promise<PromptResult>;
	closeSession: (c: Ctx, sessionId: string) => Promise<void>;
	listPersistedSessions: (c: Ctx) => Promise<PersistedSessionRecord[]>;
	getSessionEvents: (c: Ctx, sessionId: string) => Promise<PersistedSessionEvent[]>;

	// ── Preview URLs ──────────────────────────────────────────────────
	createSignedPreviewUrl: (c: Ctx, port: number, ttlSeconds: number) => Promise<SignedPreviewUrl>;
	expireSignedPreviewUrl: (c: Ctx, token: string) => Promise<void>;

	// ── Config introspection ──────────────────────────────────────────
	listMounts: (c: Ctx) => Promise<MountInfo[]>;
	listSoftware: (c: Ctx) => Promise<SoftwareInfo[]>;
}
