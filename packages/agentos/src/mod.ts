/**
 * `@rivet-dev/agentos` — the user-facing Agent OS integration for RivetKit.
 *
 * INTERIM "types-now, noop-the-runtime" STUB.
 *
 * This package owns the Agent OS-specific TypeScript surface (config schema
 * type, events, the actor-definition type, the `vm.*` action interface, and
 * the `nodeModulesMount` helper) and imports only GENERIC primitives (the
 * actor definition / registry / action context) from `rivetkit`.
 *
 * The runtime is intentionally NOT wired: `agentOs()` returns a correctly
 * typed RivetKit actor definition whose action handler bodies THROW. This
 * lets consumers (examples, downstream packages) type-check against the real
 * shape before the native runtime (dylib) lands, without pulling in any napi
 * / native module. When the dylib is ready, swap the throwing handlers for
 * the native factory builder — the public types do not change.
 */

import { actor, type ActorDefinition, event } from "rivetkit";

// Re-export the RivetKit registry builder so consumers import everything they
// need from `@rivet-dev/agentos` and never reach for `rivetkit` directly.
export { setup } from "rivetkit";

// ---------------------------------------------------------------------------
// Owned ACP / Agent OS primitive shapes
//
// These mirror the structural shape of the types that the real integration
// imports from `@rivet-dev/agent-os-core`. They are deliberately owned here
// (rather than imported) so this package stays self-contained on `rivetkit`
// + `@rivetkit/react` until the runtime is wired. Swap them for the core
// package's canonical types when the dylib lands.
// ---------------------------------------------------------------------------

/** A JSON-RPC notification frame emitted by the ACP adapter. */
export interface JsonRpcNotification {
	jsonrpc: "2.0";
	method: string;
	params?: unknown;
}

/** A JSON-RPC response frame returned by the ACP adapter. */
export interface JsonRpcResponse {
	jsonrpc: "2.0";
	id: number | string;
	result?: unknown;
	error?: { code: number; message: string; data?: unknown };
}

/** A permission request surfaced by the agent to the host. */
export interface PermissionRequest {
	/** Stable id used to reply to this specific request via `respondPermission`. */
	permissionId: string;
	/** Optional human-readable description of what is being requested. */
	description?: string;
	/** Raw ACP permission params (tool name, input, options, etc.). */
	params?: Record<string, unknown>;
	/** @deprecated legacy alias retained for older docs snippets. */
	toolName?: string;
	/** @deprecated legacy alias retained for older docs snippets. */
	rationale?: string;
	/** @deprecated legacy alias retained for older docs snippets. */
	input?: unknown;
}

/** Reply to a permission request: allow once, allow always, or reject. */
export type PermissionReply = "once" | "always" | "reject";

/** One selectable agent mode (e.g. "plan", "auto"). */
export interface SessionMode {
	id: string;
	name?: string;
	label?: string;
	description?: string;
	[key: string]: unknown;
}

/** The set of modes available to a session plus the current selection. */
export interface SessionModeState {
	currentModeId: string;
	availableModes: SessionMode[];
}

/** A configurable session option (model, thought level, etc.). */
export interface SessionConfigOption {
	id: string;
	category?: string;
	label?: string;
	description?: string;
	currentValue?: string;
	allowedValues?: Array<{ id: string; label?: string }>;
	readOnly?: boolean;
}

/** Capabilities advertised by an ACP agent during initialization. */
export interface AgentCapabilities {
	loadSession?: boolean;
	promptCapabilities?: Record<string, unknown>;
	[key: string]: unknown;
}

/** Identity metadata advertised by an ACP agent. */
export interface AgentInfo {
	name: string;
	version?: string;
	[key: string]: unknown;
}

/** A cron tick delivered to the actor. */
export interface CronEvent {
	id: string;
	schedule: string;
	firedAt: number;
}

// ---------------------------------------------------------------------------
// Filesystem primitives (mirror core's runtime-compat / agent-os shapes)
// ---------------------------------------------------------------------------

/** POSIX-style stat record returned by `stat`. */
export interface VirtualStat {
	mode: number;
	size: number;
	blocks: number;
	dev: number;
	rdev: number;
	isDirectory: boolean;
	isSymbolicLink: boolean;
	atimeMs: number;
	mtimeMs: number;
	ctimeMs: number;
	birthtimeMs: number;
	ino: number;
	nlink: number;
	uid: number;
	gid: number;
}

/** A directory entry with metadata (returned by `readdirRecursive`). */
export interface DirEntry {
	/** Absolute path to the entry. */
	path: string;
	type: "file" | "directory" | "symlink";
	size: number;
}

/** Options for `readdirRecursive`. */
export interface ReaddirRecursiveOptions {
	/** Maximum depth to recurse (0 = only immediate children). */
	maxDepth?: number;
	/** Directory names to skip. */
	exclude?: string[];
}

/** A single file in a batch write (`writeFiles`). */
export interface BatchWriteEntry {
	path: string;
	content: string | Uint8Array;
}

/** Result of a single file in a batch write. */
export interface BatchWriteResult {
	path: string;
	success: boolean;
	error?: string;
}

/** Result of a single file in a batch read (`readFiles`). */
export interface BatchReadResult {
	path: string;
	content: Uint8Array | null;
	error?: string;
}

/** A single entry in an exported root filesystem snapshot. */
export interface FilesystemEntry {
	path: string;
	type: "file" | "directory" | "symlink";
	mode: string;
	uid: number;
	gid: number;
	content?: string;
	encoding?: "utf8" | "base64";
	target?: string;
}

/** A serializable export of the VM root filesystem. */
export interface RootSnapshotExport {
	kind: "snapshot-export";
	source: {
		format: "agent-os-filesystem-snapshot-v1";
		filesystem: { entries: FilesystemEntry[] };
	};
}

// ---------------------------------------------------------------------------
// Process + shell primitives
// ---------------------------------------------------------------------------

/** Options for `exec` / `execArgv`. */
export interface ExecOptions {
	env?: Record<string, string>;
	cwd?: string;
	stdin?: string | Uint8Array;
	timeout?: number;
	captureStdio?: boolean;
}

/** Result of a synchronous `exec` / `execArgv`. */
export interface ExecResult {
	exitCode: number;
	stdout: string;
	stderr: string;
}

/** Options for `spawn`. */
export interface SpawnOptions {
	env?: Record<string, string>;
	cwd?: string;
	stdin?: string | Uint8Array;
}

/** Info about a process spawned via `spawn`. */
export interface SpawnedProcessInfo {
	pid: number;
	command: string;
	args: string[];
	running: boolean;
	exitCode: number | null;
}

/** Options for `openShell` / `connectTerminal`. */
export interface OpenShellOptions {
	command?: string;
	args?: string[];
	env?: Record<string, string>;
	cwd?: string;
	cols?: number;
	rows?: number;
}

/** Options for `connectTerminal` (a shell plus a streaming data callback). */
export interface ConnectTerminalOptions extends OpenShellOptions {
	onData?: (data: Uint8Array) => void;
}

// ---------------------------------------------------------------------------
// Networking + preview URLs
// ---------------------------------------------------------------------------

/** Options for `vmFetch` (method/headers/body for the proxied request). */
export interface VmFetchOptions {
	method?: string;
	headers?: Record<string, string>;
	body?: string | Uint8Array;
}

/** Response returned by `vmFetch`. */
export interface VmFetchResponse {
	status: number;
	statusText?: string;
	headers?: Array<[string, string]>;
	body: Uint8Array;
}

/** A time-limited, token-based preview URL for a VM port. */
export interface PreviewUrl {
	/** Signed path (including token query) to proxy to the VM port. */
	path: string;
	/** Opaque signed token authorizing the proxy. */
	token: string;
	/** Epoch millis at which the token expires. */
	expiresAt: number;
}

// ---------------------------------------------------------------------------
// Session event replay
// ---------------------------------------------------------------------------

/** An in-memory session event with its monotonic sequence number. */
export interface SequencedSessionEvent {
	sequenceNumber: number;
	notification: JsonRpcNotification;
}

/** Options for `getSequencedEvents` (replay in-memory events after `since`). */
export interface GetSequencedEventsOptions {
	/** Return only events with a sequence number greater than this value. */
	since?: number;
}

/**
 * Serializable Agent OS VM options. These fields are accepted at the top level
 * of `agentOS({ ... })` (see `AgentOsActorConfigInput`); this type collects
 * them for callers that want to build the option set as a standalone value.
 *
 * Kept structurally open: the full option set is owned by the sidecar/native
 * layer and forwarded across the boundary, so consumers configure it as data.
 */
export interface AgentOsOptions {
	software?: unknown;
	additionalInstructions?: string;
	loopbackExemptPorts?: number[];
	allowedNodeBuiltins?: string[];
	permissions?: unknown;
	rootFilesystem?: unknown;
	mounts?: unknown[];
	limits?: unknown;
	sidecar?: unknown;
	[key: string]: unknown;
}

// ---------------------------------------------------------------------------
// Mounts
// ---------------------------------------------------------------------------

/**
 * A native `host_dir` mount of a host `node_modules` directory at
 * `/root/node_modules`.
 */
export interface NodeModulesMountConfig {
	path: "/root/node_modules";
	plugin: { id: "host_dir"; config: { hostPath: string; readOnly: boolean } };
	readOnly: boolean;
}

/**
 * Mount a host `node_modules` directory into the VM at `/root/node_modules`.
 *
 * @param hostNodeModulesDir Absolute host path to a `node_modules` directory.
 * @param opts.readOnly Defaults to `true`; the mount is read-only.
 */
export function nodeModulesMount(
	hostNodeModulesDir: string,
	opts?: { readOnly?: boolean },
): NodeModulesMountConfig {
	const readOnly = opts?.readOnly ?? true;
	return {
		path: "/root/node_modules",
		plugin: {
			id: "host_dir",
			config: { hostPath: hostNodeModulesDir, readOnly },
		},
		readOnly,
	};
}

// ---------------------------------------------------------------------------
// Actor state / vars
// ---------------------------------------------------------------------------

// biome-ignore lint/complexity/noBannedTypes: empty state placeholder
export type AgentOsActorState = {};

export interface AgentOsActorVars {
	activeSessionIds: Set<string>;
	activeProcesses: Set<number>;
	activeShells: Set<string>;
	sessions: Set<string>;
}

// ---------------------------------------------------------------------------
// Event payloads + schema map
// ---------------------------------------------------------------------------

export interface SessionEventPayload {
	sessionId: string;
	event: JsonRpcNotification;
}

export interface PermissionRequestPayload {
	sessionId: string;
	request: PermissionRequest;
}

export type VmBootedPayload = Record<string, never>;

export interface VmShutdownPayload {
	reason: "sleep" | "destroy" | "error";
}

export interface ProcessOutputPayload {
	pid: number;
	stream: "stdout" | "stderr";
	data: Uint8Array;
}

export interface ProcessExitPayload {
	pid: number;
	exitCode: number;
}

export interface ShellDataPayload {
	shellId: string;
	data: Uint8Array;
}

export interface CronEventPayload {
	event: CronEvent;
}

/**
 * Event schema map consumed by the actor `events` config.
 *
 * Built with RivetKit's `event<T>()` helper so each entry is a valid
 * `EventSchema` (an `EventTypeToken` carrying the payload type). This lets the
 * actor definition satisfy RivetKit's `EventSchemaConfig` while
 * `conn.on(name, cb)` infers the payload for each event with no casts.
 */
const agentOsEvents = {
	sessionEvent: event<SessionEventPayload>(),
	permissionRequest: event<PermissionRequestPayload>(),
	vmBooted: event<VmBootedPayload>(),
	vmShutdown: event<VmShutdownPayload>(),
	processOutput: event<ProcessOutputPayload>(),
	processExit: event<ProcessExitPayload>(),
	shellData: event<ShellDataPayload>(),
	cronEvent: event<CronEventPayload>(),
};

/** Event schema map type for the actor `events` config (payload-inferring). */
export type AgentOsEvents = typeof agentOsEvents;

// ---------------------------------------------------------------------------
// Session + prompt records
// ---------------------------------------------------------------------------

export interface PromptResult {
	response: JsonRpcResponse;
	text: string;
}

export interface SessionRecord {
	sessionId: string;
	agentType: string;
	capabilities: AgentCapabilities;
	agentInfo: AgentInfo | null;
}

export interface PersistedSessionRecord extends SessionRecord {
	createdAt: number;
}

export interface PersistedSessionEvent {
	sessionId: string;
	seq: number;
	event: JsonRpcNotification;
	createdAt: number;
}

// ---------------------------------------------------------------------------
// Cron
// ---------------------------------------------------------------------------

export type SerializableCronAction =
	| {
			type: "session";
			agentType: string;
			prompt: string;
			/** Session options applied when the cron fires (cwd, env, etc.). */
			options?: CreateSessionOptions;
	  }
	| { type: "exec"; command: string; args?: string[] };

export interface SerializableCronJobOptions {
	id?: string;
	schedule: string;
	action: SerializableCronAction;
	overlap?: "allow" | "skip" | "queue";
}

/** Result of scheduling a cron job. */
export interface ScheduledCronJob {
	id: string;
}

/** Info about a registered cron job (returned by `listCronJobs`). */
export interface CronJobInfo {
	id: string;
	schedule: string;
	action: SerializableCronAction;
	overlap: "allow" | "skip" | "queue";
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/**
 * Input config passed to `agentOS(...)`.
 *
 * The VM option fields live at the TOP LEVEL (no nested `options` key), so
 * callers write `agentOS({ software: [...], additionalInstructions: "..." })`.
 */
export interface AgentOsActorConfigInput<TConnParams = undefined> {
	/**
	 * Software packages to install in the VM. Pass the imported packages
	 * directly (e.g. `software: [common, pi]`); kept loosely typed so any
	 * software package import type-checks.
	 */
	software?: unknown[];
	/** Additional instructions appended to the base OS instructions. */
	additionalInstructions?: string;
	/** Ports exempt from the loopback-only network restriction. */
	loopbackExemptPorts?: number[];
	/** Node.js builtins the guest is allowed to import. */
	allowedNodeBuiltins?: string[];
	/** Permission policy applied to the VM. */
	permissions?: unknown;
	/** Root filesystem configuration / snapshot. */
	rootFilesystem?: unknown;
	/** Host-backed mounts exposed inside the VM. */
	mounts?: unknown[];
	/** Bindings exposed to agents as CLI commands inside the VM. */
	bindings?: unknown[];
	/** Per-VM resource limits. */
	limits?: unknown;
	/** Low-level runtime configuration. */
	sidecar?: unknown;
	preview?: {
		defaultExpiresInSeconds?: number;
		maxExpiresInSeconds?: number;
	};
	onSessionEvent?: (
		sessionId: string,
		event: JsonRpcNotification,
	) => void | Promise<void>;
	onPermissionRequest?: (
		sessionId: string,
		request: PermissionRequest,
	) => void | Promise<void>;
	// Reserved for parity with the native config's typed connection params.
	__connParams?: TConnParams;
}

/** Parsed config (after defaults applied). */
export interface AgentOsActorConfig<TConnParams = undefined>
	extends AgentOsActorConfigInput<TConnParams> {
	preview: { defaultExpiresInSeconds: number; maxExpiresInSeconds: number };
}

// ---------------------------------------------------------------------------
// MCP servers + session options (mirrors @rivet-dev/agentos-core's
// CreateSessionOptions / McpServerConfig)
// ---------------------------------------------------------------------------

/** A local MCP server launched as a child process inside the VM. */
export interface McpServerConfigLocal {
	type: "local";
	/** Command to launch the MCP server. */
	command: string;
	/** Arguments for the command. */
	args?: string[];
	/** Environment variables for the server process. */
	env?: Record<string, string>;
}

/** A remote MCP server reachable over HTTP. */
export interface McpServerConfigRemote {
	type: "remote";
	/** URL of the remote MCP server. */
	url: string;
	/** HTTP headers to include in requests to the server. */
	headers?: Record<string, string>;
}

export type McpServerConfig = McpServerConfigLocal | McpServerConfigRemote;

/** Options accepted by `createSession(agentType, options?)`. */
export interface CreateSessionOptions {
	/** Working directory for the agent session inside the VM. Defaults to /workspace. */
	cwd?: string;
	/** Environment variables passed to the agent process (e.g. API keys). */
	env?: Record<string, string>;
	/** MCP servers made available to the agent during the session. */
	mcpServers?: McpServerConfig[];
	/** Skip OS instructions injection entirely (default false). */
	skipOsInstructions?: boolean;
	/** Additional instructions appended to the base OS instructions. */
	additionalInstructions?: string;
}

// ---------------------------------------------------------------------------
// vm.* action surface
//
// The real native factory exposes these as `any`; this stub gives them real
// types so consumers (and tsc) exercise the intended shape.
// ---------------------------------------------------------------------------

/**
 * The `vm.*` action surface as the SERVER defines it. Each handler receives
 * the RivetKit action context (`c`) as its first argument; RivetKit strips
 * that context when projecting these onto the client handle, so callers invoke
 * e.g. `handle.createSession("claude", { env: { ANTHROPIC_API_KEY } })`.
 *
 * The string index signature satisfies RivetKit's `Actions` constraint.
 */
export interface VmActions {
	// ── Sessions ────────────────────────────────────────────────────
	/** Create an ACP session against a registered agent and return its record. */
	createSession(
		c: any,
		agentType: string,
		options?: CreateSessionOptions,
	): Promise<SessionRecord>;
	/** Send a prompt to an existing session and accumulate the agent's reply. */
	sendPrompt(c: any, sessionId: string, prompt: string): Promise<PromptResult>;
	/** Alias of `sendPrompt`: send a prompt and accumulate the agent's reply. */
	prompt(c: any, sessionId: string, text: string): Promise<PromptResult>;
	/** Cancel the in-flight prompt for a session (leaves the session open). */
	cancelPrompt(c: any, sessionId: string): Promise<JsonRpcResponse>;
	/** Cancel ongoing agent work for a session. */
	cancelSession(c: any, sessionId: string): Promise<JsonRpcResponse>;
	/** List the sessions currently tracked by this VM. */
	listSessions(c: any): Promise<SessionRecord[]>;
	/** Tear down an ACP session abruptly. */
	closeSession(c: any, sessionId: string): Promise<void>;
	/** Gracefully destroy a session (cancel pending work, then close). */
	destroySession(c: any, sessionId: string): Promise<void>;
	/** Reply to a pending permission request raised by the agent. */
	respondPermission(
		c: any,
		sessionId: string,
		permissionId: string,
		reply: PermissionReply,
	): Promise<JsonRpcResponse>;
	/** Switch the active mode (e.g. "plan", "auto") for a session. */
	setMode(c: any, sessionId: string, modeId: string): Promise<JsonRpcResponse>;
	/** Switch the active model for a session. */
	setModel(c: any, sessionId: string, model: string): Promise<JsonRpcResponse>;
	/** Switch the thought/reasoning level for a session. */
	setThoughtLevel(
		c: any,
		sessionId: string,
		level: string,
	): Promise<JsonRpcResponse>;
	/** Get the available + current modes for a session. */
	getModes(c: any, sessionId: string): Promise<SessionModeState | null>;
	/** Get the configurable options (model, thought level, ...) for a session. */
	getConfigOptions(c: any, sessionId: string): Promise<SessionConfigOption[]>;

	// ── Session event replay ────────────────────────────────────────
	/** Replay in-memory session events (live reconnection); newer than `since`. */
	getSequencedEvents(
		c: any,
		sessionId: string,
		options?: GetSequencedEventsOptions,
	): Promise<SequencedSessionEvent[]>;
	/** Replay persisted session events from durable storage (transcript history). */
	getSessionEvents(
		c: any,
		sessionId: string,
	): Promise<PersistedSessionEvent[]>;
	/** List persisted session records (including for VMs not currently running). */
	listPersistedSessions(c: any): Promise<PersistedSessionRecord[]>;

	// ── Processes ───────────────────────────────────────────────────
	/** Run a command line to completion and capture its output. */
	exec(c: any, command: string, options?: ExecOptions): Promise<ExecResult>;
	/** Run a command (argv form, no shell) to completion and capture output. */
	execArgv(
		c: any,
		command: string,
		args?: readonly string[],
		options?: ExecOptions,
	): Promise<ExecResult>;
	/** Spawn a long-running process and return its pid. */
	spawn(
		c: any,
		command: string,
		args?: string[],
		options?: SpawnOptions,
	): Promise<{ pid: number }>;
	/** Spawn a process inside the VM and return its pid. */
	spawnProcess(c: any, command: string, args?: string[]): Promise<number>;
	/** List processes spawned via `spawn`. */
	listProcesses(c: any): Promise<SpawnedProcessInfo[]>;
	/** Get info about a single spawned process. */
	getProcess(c: any, pid: number): Promise<SpawnedProcessInfo>;
	/** Write data to a spawned process's stdin. */
	writeProcessStdin(
		c: any,
		pid: number,
		data: string | Uint8Array,
	): Promise<void>;
	/** Close a spawned process's stdin stream. */
	closeProcessStdin(c: any, pid: number): Promise<void>;
	/** Wait for a process to exit and return its exit code. */
	waitProcess(c: any, pid: number): Promise<number>;
	/** Gracefully stop a process (SIGTERM). */
	stopProcess(c: any, pid: number): Promise<void>;
	/** Force-kill a running process by pid (SIGKILL). */
	killProcess(c: any, pid: number): Promise<void>;

	// ── Shells / PTYs ───────────────────────────────────────────────
	/** Open an interactive shell (PTY) and return its id. */
	openShell(c: any, options?: OpenShellOptions): Promise<{ shellId: string }>;
	/** Connect a terminal (PTY) to the VM and return its pid. */
	connectTerminal(c: any, options?: ConnectTerminalOptions): Promise<number>;
	/** Write input to a shell's PTY. */
	writeShell(c: any, shellId: string, data: string | Uint8Array): Promise<void>;
	/** Resize a shell's PTY. */
	resizeShell(
		c: any,
		shellId: string,
		cols: number,
		rows: number,
	): Promise<void>;
	/** Close a shell and free its PTY. */
	closeShell(c: any, shellId: string): Promise<void>;

	// ── Filesystem ──────────────────────────────────────────────────
	/** Read a file from the VM filesystem. */
	readFile(c: any, path: string): Promise<Uint8Array>;
	/** Write a file into the VM filesystem. */
	writeFile(c: any, path: string, content: Uint8Array | string): Promise<void>;
	/** Write several files in one call, reporting per-file success. */
	writeFiles(c: any, entries: BatchWriteEntry[]): Promise<BatchWriteResult[]>;
	/** Read several files in one call, reporting per-file content/errors. */
	readFiles(c: any, paths: string[]): Promise<BatchReadResult[]>;
	/** Create a directory (optionally recursive). */
	mkdir(
		c: any,
		path: string,
		options?: { recursive?: boolean },
	): Promise<void>;
	/** List the immediate entries of a directory. */
	readdir(c: any, path: string): Promise<string[]>;
	/** Recursively list directory entries with metadata. */
	readdirRecursive(
		c: any,
		path: string,
		options?: ReaddirRecursiveOptions,
	): Promise<DirEntry[]>;
	/** Stat a path. */
	stat(c: any, path: string): Promise<VirtualStat>;
	/** Check whether a path exists. */
	exists(c: any, path: string): Promise<boolean>;
	/** Move/rename a path. */
	move(c: any, from: string, to: string): Promise<void>;
	/** Delete a path (optionally recursive). */
	delete(
		c: any,
		path: string,
		options?: { recursive?: boolean },
	): Promise<void>;
	/** Export a serializable snapshot of the VM root filesystem. */
	snapshotRootFilesystem(c: any): Promise<RootSnapshotExport>;

	// ── Networking + preview URLs ───────────────────────────────────
	/** Proxy an HTTP request to a port inside the VM and return the response. */
	vmFetch(
		c: any,
		port: number,
		path: string,
		options?: VmFetchOptions,
	): Promise<VmFetchResponse>;
	/** Create a time-limited, token-based preview URL for a VM port. */
	createSignedPreviewUrl(
		c: any,
		port: number,
		expiresInSeconds?: number,
	): Promise<PreviewUrl>;

	// ── Cron ────────────────────────────────────────────────────────
	/** Schedule a serializable cron job inside the VM. */
	scheduleCron(
		c: any,
		options: SerializableCronJobOptions,
	): Promise<ScheduledCronJob>;
	/** List the cron jobs registered on this VM. */
	listCronJobs(c: any): Promise<CronJobInfo[]>;
	/** Cancel a registered cron job by id. */
	cancelCronJob(c: any, id: string): Promise<void>;

	/** Index signature required by RivetKit's `Actions` constraint. */
	[action: string]: (c: any, ...args: any[]) => any;
}

// ---------------------------------------------------------------------------
// Actor definition
// ---------------------------------------------------------------------------

/**
 * The `agentOS(...)` actor definition type. Mirrors RivetKit's generic
 * `ActorDefinition` with the Agent OS state/vars/events/actions bound in.
 */
export type AgentOsActorDefinition<TConnParams = undefined> = ActorDefinition<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	any,
	AgentOsEvents,
	Record<never, never>,
	VmActions
>;

const RUNTIME_NOT_WIRED =
	"agent-os runtime not yet wired — dylib pending";

/**
 * Build the Agent OS actor definition.
 *
 * STUB: returns a correctly typed RivetKit actor definition whose action
 * handlers throw. No native/napi module is imported. Replace the throwing
 * handlers with the native factory builder once the runtime ships; the public
 * type surface is stable.
 */
export function agentOS<TConnParams = undefined>(
	// Accepted for type-surface parity; ignored by the stub.
	_config: AgentOsActorConfigInput<TConnParams>,
): AgentOsActorDefinition<TConnParams> {
	const notWired = () => {
		throw new Error(RUNTIME_NOT_WIRED);
	};

	const definition = actor({
		state: {} as AgentOsActorState,
		createVars: (): AgentOsActorVars => ({
			activeSessionIds: new Set<string>(),
			activeProcesses: new Set<number>(),
			activeShells: new Set<string>(),
			sessions: new Set<string>(),
		}),
		events: agentOsEvents,
		actions: {
			// Sessions
			createSession: notWired,
			sendPrompt: notWired,
			prompt: notWired,
			cancelPrompt: notWired,
			cancelSession: notWired,
			listSessions: notWired,
			closeSession: notWired,
			destroySession: notWired,
			respondPermission: notWired,
			setMode: notWired,
			setModel: notWired,
			setThoughtLevel: notWired,
			getModes: notWired,
			getConfigOptions: notWired,
			// Session event replay
			getSequencedEvents: notWired,
			getSessionEvents: notWired,
			listPersistedSessions: notWired,
			// Processes
			exec: notWired,
			execArgv: notWired,
			spawn: notWired,
			spawnProcess: notWired,
			listProcesses: notWired,
			getProcess: notWired,
			writeProcessStdin: notWired,
			closeProcessStdin: notWired,
			waitProcess: notWired,
			stopProcess: notWired,
			killProcess: notWired,
			// Shells
			openShell: notWired,
			connectTerminal: notWired,
			writeShell: notWired,
			resizeShell: notWired,
			closeShell: notWired,
			// Filesystem
			readFile: notWired,
			writeFile: notWired,
			writeFiles: notWired,
			readFiles: notWired,
			mkdir: notWired,
			readdir: notWired,
			readdirRecursive: notWired,
			stat: notWired,
			exists: notWired,
			move: notWired,
			delete: notWired,
			snapshotRootFilesystem: notWired,
			// Networking + preview URLs
			vmFetch: notWired,
			createSignedPreviewUrl: notWired,
			// Cron
			scheduleCron: notWired,
			listCronJobs: notWired,
			cancelCronJob: notWired,
		},
	});

	return definition as unknown as AgentOsActorDefinition<TConnParams>;
}
