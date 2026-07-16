import type {
	AgentCapabilities,
	AgentExitEvent,
	AgentInfo,
	JsonRpcNotification,
	PermissionRequest,
} from "@rivet-dev/agentos-core";

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

// --- Event schema map (used by actor() events config) ---

export interface AgentOsEvents {
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
	action: SerializableCronAction;
	overlap: "allow" | "skip" | "queue";
	lastRun?: string;
	nextRun?: string;
	runCount: number;
	running: boolean;
}
