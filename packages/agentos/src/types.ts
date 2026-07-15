import type {
	AgentCapabilities,
	AgentExitEvent,
	AgentInfo,
	AgentOs,
	JsonRpcNotification,
	JsonRpcResponse,
	PermissionReply,
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

/** Broadcast after a successful `respondPermission` so every connected client
 * (e.g. a second inspector viewer) can drop its pending card. */
export interface PermissionResolvedPayload {
	sessionId: string;
	permissionId: string;
	reply: PermissionReply;
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

// --- Event schema map (used by actor() events config) ---

export interface AgentOsEvents {
	sessionEvent: SessionEventPayload;
	permissionRequest: PermissionRequestPayload;
	permissionResolved: PermissionResolvedPayload;
	agentCrashed: AgentCrashedPayload;
	vmBooted: VmBootedPayload;
	vmShutdown: VmShutdownPayload;
	processOutput: ProcessOutputPayload;
	processExit: ProcessExitPayload;
	shellData: ShellDataPayload;
	/** Shell stderr (the dedicated channel backing the TS `onStderr` option). */
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
