import type { OpenSessionInput } from "../session-api.js";
import type { AgentType } from "../types.js";

export type CronAction =
	| {
			type: "session";
			agentType: AgentType;
			prompt: string;
			options?: Omit<OpenSessionInput, "agent" | "sessionId">;
	  }
	| { type: "exec"; command: string; args?: string[] }
	| { type: "callback"; fn: () => void | Promise<void> };

/**
 * Serializable description of a scheduled action. Callback jobs deliberately
 * expose only their kind: the host closure is execution state, not job data.
 */
export type CronActionInfo =
	| {
			type: "session";
			agentType: AgentType;
			prompt: string;
			options?: Omit<OpenSessionInput, "agent" | "sessionId">;
	  }
	| { type: "exec"; command: string; args?: string[] }
	| { type: "callback" };

export interface CronJobOptions {
	/** Optional ID. Auto-generated UUID if omitted. */
	id?: string;
	/** Standard 5-field cron expression or ISO 8601 timestamp for one-shot. */
	schedule: string;
	/** What to do when the schedule fires. */
	action: CronAction;
	/** What to do if previous execution is still running. Default: 'allow'. */
	overlap?: "allow" | "skip" | "queue";
}

export interface CronJob {
	id: string;
	cancel(): void;
}

export interface CronJobInfo {
	id: string;
	schedule: string;
	action: CronActionInfo;
	overlap: "allow" | "skip" | "queue";
	lastRun?: string;
	nextRun?: string;
	runCount: number;
	running: boolean;
}

export type CronEvent =
	| { type: "cron:fire"; jobId: string; time: string }
	| { type: "cron:complete"; jobId: string; time: string; durationMs: number }
	| { type: "cron:error"; jobId: string; time: string; error: string };

export type CronEventHandler = (event: CronEvent) => void;
