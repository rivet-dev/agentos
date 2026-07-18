import type {
	AgentExitEvent,
	CronEvent,
	CronJobInfo,
	ProcessExit,
	ProcessOutput,
	SessionStreamEntry,
	ShellData,
	ShellExit,
} from "@rivet-dev/agentos-core";

export type VmBootedPayload = Record<string, never>;

export interface VmShutdownPayload {
	reason: "sleep" | "destroy" | "error";
}

export type ProcessOutputPayload = ProcessOutput;
export type ProcessExitPayload = ProcessExit;
export type ShellDataPayload = ShellData;
export type ShellExitPayload = ShellExit;
export type SerializableCronEvent = CronEvent;

// --- Event schema map (used by actor() events config) ---

export interface AgentOsEvents {
	sessionEvent: SessionStreamEntry;
	agentExit: AgentExitEvent;
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
	cronEvent: SerializableCronEvent;
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

export type SerializableCronJobInfo = CronJobInfo;
