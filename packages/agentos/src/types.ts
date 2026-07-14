import type {
	AgentCapabilities,
	AgentExitEvent,
	AgentInfo,
	AgentOs,
	JsonRpcNotification,
	JsonRpcResponse,
	PermissionRequest,
} from "@rivet-dev/agentos-core";
import type { ActionContext } from "rivetkit";

// --- Actor state (persisted across sleep/wake) ---

// biome-ignore lint/complexity/noBannedTypes: empty state placeholder, consumers extend via generics
export type AgentOsActorState = {};

// --- Actor vars (ephemeral, recreated on wake) ---

export interface AgentOsActorVars {
	agentOs: AgentOs | null;
	activeSessionIds: Set<string>;
	activeProcesses: Set<number>;
	activeHooks: Set<Promise<void>>;
	activeShells: Set<string>;
	sessions: Set<string>;
}

// --- Event payloads ---

export interface SessionEventPayload {
	sessionId: string;
	event: JsonRpcNotification;
}

export interface PermissionRequestPayload {
	sessionId: string;
	request: PermissionRequest;
}

export interface AgentCrashedPayload {
	sessionId: string;
	event: AgentExitEvent;
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

export interface ShellExitPayload {
	shellId: string;
	exitCode: number;
}

export type SerializableCronEvent =
	| { type: "cron:fire"; jobId: string; time: number }
	| { type: "cron:complete"; jobId: string; time: number; durationMs: number }
	| { type: "cron:error"; jobId: string; time: number; error: string };

export interface CronEventPayload {
	event: SerializableCronEvent;
}

export interface StreamErrorPayload {
	scope: "process" | "shell" | "session" | "cron";
	id?: string;
	code: "event_stream_lagged" | "event_stream_closed" | "stream_failed";
	skipped?: number;
	message: string;
}

// --- Event schema map (used by actor() events config) ---

export interface AgentOsEvents {
	/** A bounded actor stream lost continuity or closed before its terminal event. */
	streamError: StreamErrorPayload;
	sessionEvent: SessionEventPayload;
	permissionRequest: PermissionRequestPayload;
	agentCrashed: AgentCrashedPayload;
	vmBooted: VmBootedPayload;
	vmShutdown: VmShutdownPayload;
	processOutput: ProcessOutputPayload;
	processExit: ProcessExitPayload;
	/** Ordered PTY output containing stdout and stderr exactly once. */
	shellData: ShellDataPayload;
	/** Optional stderr-only diagnostic tap; do not render it with `shellData`. */
	shellStderr: ShellDataPayload;
	/** Shell process exit (mirrors `waitShell` resolution). */
	shellExit: ShellExitPayload;
	cronEvent: CronEventPayload;
}

// --- Prompt result ---

/** Result from sendPrompt. */
export interface PromptResult {
	/** Raw JSON-RPC response from the ACP adapter. */
	response: JsonRpcResponse;
	/** Accumulated agent text output from streamed message chunks. */
	text: string;
}

// --- Session serialization ---

export interface SessionRecord {
	sessionId: string;
	agentType: string;
	capabilities: AgentCapabilities;
	agentInfo: AgentInfo | null;
}

// --- Persisted session types ---

export interface PersistedSessionRecord {
	sessionId: string;
	agentType: string;
	createdAt: number;
	status: "running" | "idle";
}

export interface PersistedSessionEvent {
	sessionId: string;
	seq: number;
	event: JsonRpcNotification;
	createdAt: number;
}

// --- Serializable cron action (excludes callback type) ---

export type SerializableCronAction =
	| { type: "session"; agentType: string; prompt: string; cwd?: string }
	| { type: "exec"; command: string; args?: string[] };

export interface SerializableCronJobOptions {
	id?: string;
	schedule: string;
	action: SerializableCronAction;
	overlap?: "allow" | "skip" | "queue";
}

export interface SerializableCronJobInfo {
	id: string;
	schedule: string;
	overlap: "allow" | "skip" | "queue";
	lastRun?: number;
	nextRun?: number;
}

// --- Action context alias ---

export type AgentOsActionContext<TConnParams = undefined> = ActionContext<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	any
>;
