import { randomUUID } from "node:crypto";
import type { AgentOs } from "../agent-os.js";
import {
	resolveSchedule,
	validateScheduleForRegistration,
} from "./parse-schedule.js";
import type { ScheduleDriver, ScheduleHandle } from "./schedule-driver.js";
import type {
	CronAction,
	CronActionInfo,
	CronEvent,
	CronEventHandler,
	CronJob,
	CronJobInfo,
	CronJobOptions,
} from "./types.js";

function describeAction(action: CronAction): CronActionInfo {
	switch (action.type) {
		case "session":
			return {
				type: "session",
				agentType: action.agentType,
				prompt: action.prompt,
				...(action.options === undefined
					? {}
					: { options: structuredClone(action.options) }),
			};
		case "exec":
			return {
				type: "exec",
				command: action.command,
				...(action.args === undefined ? {} : { args: [...action.args] }),
			};
		case "callback":
			return { type: "callback" };
	}
}

interface CronJobState {
	id: string;
	schedule: string;
	action: CronAction;
	overlap: "allow" | "skip" | "queue";
	handle: ScheduleHandle;
	lastRun?: Date;
	nextRun?: Date;
	runCount: number;
	running: boolean;
	queued: boolean;
}

/**
 * Compute the next fire time for a schedule string. Returns undefined if
 * the schedule is a one-shot ISO timestamp in the past or if croner
 * cannot determine a next run.
 */
function computeNextTime(schedule: string): Date | undefined {
	return resolveSchedule(schedule).nextRun;
}

/**
 * Internal class that bridges ScheduleDriver and AgentOs. Owns the job
 * registry, executes actions, and emits lifecycle events.
 */
export class CronManager {
	private jobs = new Map<string, CronJobState>();
	private driver: ScheduleDriver;
	private vm: AgentOs;
	private listeners: CronEventHandler[] = [];

	constructor(vm: AgentOs, driver: ScheduleDriver) {
		this.vm = vm;
		this.driver = driver;
	}

	schedule(options: CronJobOptions): CronJob {
		const id = options.id ?? randomUUID();
		const overlap = options.overlap ?? "allow";
		const resolved = validateScheduleForRegistration(options.schedule);

		const handle = this.driver.schedule({
			id,
			schedule: options.schedule,
			callback: () => this.executeJob(id),
		});

		const state: CronJobState = {
			id,
			schedule: options.schedule,
			action: options.action,
			overlap,
			handle,
			lastRun: undefined,
			nextRun: resolved.nextRun,
			runCount: 0,
			running: false,
			queued: false,
		};

		this.jobs.set(id, state);
		return { id, cancel: () => this.cancel(id) };
	}

	cancel(id: string): void {
		const state = this.jobs.get(id);
		if (!state) return;
		this.driver.cancel(state.handle);
		this.jobs.delete(id);
	}

	list(): CronJobInfo[] {
		const result: CronJobInfo[] = [];
		for (const state of this.jobs.values()) {
			result.push({
				id: state.id,
				schedule: state.schedule,
				action: describeAction(state.action),
				overlap: state.overlap,
				lastRun: state.lastRun?.toISOString(),
				nextRun: state.nextRun?.toISOString(),
				runCount: state.runCount,
				running: state.running,
			});
		}
		return result;
	}

	onEvent(handler: CronEventHandler): void {
		this.listeners.push(handler);
	}

	dispose(): void {
		for (const state of this.jobs.values()) {
			this.driver.cancel(state.handle);
		}
		this.jobs.clear();
		this.driver.dispose();
	}

	private emit(event: CronEvent): void {
		for (const handler of this.listeners) {
			try {
				handler(event);
			} catch {
				// Event handler errors must not crash the manager.
			}
		}
	}

	private async executeJob(id: string): Promise<void> {
		const state = this.jobs.get(id);
		if (!state) return;

		// Overlap policy.
		if (state.running && state.overlap === "skip") {
			return;
		}
		if (state.running && state.overlap === "queue") {
			state.queued = true;
			return;
		}

		state.running = true;
		state.lastRun = new Date();
		state.runCount++;

		this.emit({
			type: "cron:fire",
			jobId: state.id,
			time: new Date().toISOString(),
		});

		const startTime = Date.now();
		try {
			await this.runAction(state.action);
			this.emit({
				type: "cron:complete",
				jobId: state.id,
				time: new Date().toISOString(),
				durationMs: Date.now() - startTime,
			});
		} catch (error) {
			this.emit({
				type: "cron:error",
				jobId: state.id,
				time: new Date().toISOString(),
				error: error instanceof Error ? error.message : String(error),
			});
		} finally {
			state.running = false;
			state.nextRun = computeNextTime(state.schedule);

			// Process queued execution.
			if (state.queued) {
				state.queued = false;
				void this.executeJob(id);
			}
		}
	}

	private async runAction(action: CronAction): Promise<void> {
		switch (action.type) {
			case "session": {
				const sessionId = `cron-${randomUUID()}`;
				await this.vm.openSession({
					...action.options,
					sessionId,
					agent: action.agentType,
				});
				try {
					await this.vm.prompt({
						sessionId,
						content: [{ type: "text", text: action.prompt }],
					});
				} finally {
					await this.vm.deleteSession({ sessionId });
				}
				break;
			}
			case "exec": {
				await this.vm.execArgv(action.command, action.args ?? []);
				break;
			}
			case "callback": {
				await action.fn();
				break;
			}
		}
	}
}
