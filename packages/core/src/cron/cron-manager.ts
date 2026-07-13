import type {
	AuthenticatedSession,
	CreatedVm,
	SidecarCronAlarm,
	SidecarCronDispatch,
	SidecarCronEventRecord,
	SidecarCronJobEntry,
	SidecarCronRun,
	SidecarProcess,
} from "../sidecar/native-process-client.js";
import { InvalidScheduleError, PastScheduleError } from "./errors.js";
import type {
	CronAction,
	CronEvent,
	CronEventHandler,
	CronJob,
	CronJobInfo,
	CronJobOptions,
} from "./types.js";

const MAX_TIMER_DELAY_MS = 2_147_483_647;

type WireCronAction =
	| Exclude<CronAction, { type: "callback" }>
	| { type: "callback"; callbackId: string };

interface CallbackRoute {
	fn: () => void | Promise<void>;
	scheduled: boolean;
	activeRuns: number;
}

interface CronAlarmDriver {
	set(
		alarm: SidecarCronAlarm,
		wake: (generation: number) => Promise<void>,
	): void;
	dispose(): void;
}

class TimerCronAlarmDriver implements CronAlarmDriver {
	private timer: ReturnType<typeof setTimeout> | null = null;

	set(
		alarm: SidecarCronAlarm,
		wake: (generation: number) => Promise<void>,
	): void {
		this.clear();
		if (alarm.nextAlarmMs === undefined) return;

		const delay = Math.min(
			MAX_TIMER_DELAY_MS,
			Math.max(0, alarm.nextAlarmMs - Date.now()),
		);
		this.timer = setTimeout(() => {
			this.timer = null;
			if (delay === MAX_TIMER_DELAY_MS) {
				this.set(alarm, wake);
				return;
			}
			void wake(alarm.generation).catch((error) => {
				console.error("[agent-os] cron wake failed", error);
			});
		}, delay);
		this.timer.unref?.();
	}

	dispose(): void {
		this.clear();
	}

	private clear(): void {
		if (this.timer !== null) {
			clearTimeout(this.timer);
			this.timer = null;
		}
	}
}

interface CronTransport {
	scheduleCron: SidecarProcess["scheduleCron"];
	listCronJobs: SidecarProcess["listCronJobs"];
	cancelCronJob: SidecarProcess["cancelCronJob"];
	wakeCron: SidecarProcess["wakeCron"];
	completeCronRun: SidecarProcess["completeCronRun"];
}

/**
 * Thin cron host adapter. The sidecar owns grammar, defaults, job/run state,
 * overlap policy, counts, missed-fire coalescing, and alarm generations. This
 * class only arms the host clock and routes actions containing host resources.
 */
export class CronManager {
	private readonly listeners = new Set<CronEventHandler>();
	private readonly callbacks = new Map<string, CallbackRoute>();
	private readonly callbackByJob = new Map<string, string>();
	private callbackSequence = 0;
	private alarmGeneration = 0;
	private disposed = false;

	constructor(
		private readonly transport: CronTransport,
		private readonly session: AuthenticatedSession,
		private readonly sidecarVm: CreatedVm,
		private readonly alarmDriver: CronAlarmDriver = new TimerCronAlarmDriver(),
	) {}

	async schedule(options: CronJobOptions): Promise<CronJob> {
		this.ensureActive();
		let callbackId: string | undefined;
		const action = (() => {
			if (options.action.type !== "callback") return options.action;
			callbackId = this.allocateCallbackId();
			this.callbacks.set(callbackId, {
				fn: options.action.fn,
				scheduled: false,
				activeRuns: 0,
			});
			return { type: "callback", callbackId } satisfies WireCronAction;
		})();

		try {
			const response = await this.transport.scheduleCron(
				this.session,
				this.sidecarVm,
				{
					...(options.id === undefined ? {} : { id: options.id }),
					schedule: options.schedule,
					action,
					...(options.overlap === undefined
						? {}
						: { overlap: options.overlap }),
				},
			);

			this.replaceJobCallback(response.id, callbackId);
			this.applyAlarm(response.alarm);
			return {
				id: response.id,
				cancel: () => this.cancel(response.id),
			};
		} catch (error) {
			if (callbackId !== undefined) this.releaseCallback(callbackId);
			throw normalizeScheduleError(options.schedule, error);
		}
	}

	async cancel(id: string): Promise<void> {
		this.ensureActive();
		const response = await this.transport.cancelCronJob(
			this.session,
			this.sidecarVm,
			id,
		);
		if (response.cancelled) this.replaceJobCallback(id, undefined);
		this.applyAlarm(response.alarm);
	}

	async list(): Promise<CronJobInfo[]> {
		this.ensureActive();
		const response = await this.transport.listCronJobs(
			this.session,
			this.sidecarVm,
		);
		this.applyAlarm(response.alarm);
		return response.jobs.map((job) => this.toJobInfo(job));
	}

	onEvent(handler: CronEventHandler): void {
		this.ensureActive();
		this.listeners.add(handler);
	}

	dispose(): void {
		if (this.disposed) return;
		this.disposed = true;
		this.alarmDriver.dispose();
		this.listeners.clear();
		this.callbacks.clear();
		this.callbackByJob.clear();
	}

	private async wake(generation: number): Promise<void> {
		if (this.disposed) return;
		const dispatch = await this.transport.wakeCron(
			this.session,
			this.sidecarVm,
			generation,
		);
		this.consumeDispatch(dispatch);
	}

	consumeDispatch(dispatch: SidecarCronDispatch): void {
		if (this.disposed) return;
		this.applyAlarm(dispatch.alarm);
		for (const event of dispatch.events) this.emit(event);
		for (const run of dispatch.runs) {
			void this.executeRun(run).catch((error) => {
				console.error("[agent-os] cron completion failed", error);
			});
		}
	}

	private async executeRun(run: SidecarCronRun): Promise<void> {
		let errorMessage: string | undefined;
		let callbackId: string | undefined;
		try {
			const action = decodeWireAction(run.action);
			if (action.type !== "callback") {
				throw new Error(
					`sidecar returned non-host cron action to client: ${action.type}`,
				);
			}
			callbackId = action.callbackId;
			const route = this.callbacks.get(callbackId);
			if (!route) {
				throw new Error(`cron callback route not found: ${callbackId}`);
			}
			route.activeRuns++;
			await route.fn();
		} catch (error) {
			errorMessage = error instanceof Error ? error.message : String(error);
		} finally {
			if (callbackId !== undefined) {
				const route = this.callbacks.get(callbackId);
				if (route) {
					route.activeRuns = Math.max(0, route.activeRuns - 1);
					this.releaseCallback(callbackId);
				}
			}
		}

		// VM disposal removes the sidecar scheduler and its active runs. Do not
		// issue a completion request against a transport that is being torn down.
		if (this.disposed) return;
		const dispatch = await this.transport.completeCronRun(
			this.session,
			this.sidecarVm,
			run.runId,
			errorMessage,
		);
		this.consumeDispatch(dispatch);
	}

	private toJobInfo(job: SidecarCronJobEntry): CronJobInfo {
		const wireAction = decodeWireAction(job.action);
		const action: CronAction =
			wireAction.type === "callback"
				? {
						type: "callback",
						fn:
							this.callbacks.get(wireAction.callbackId)?.fn ?? missingCallback,
					}
				: (wireAction as CronAction);
		return {
			id: job.id,
			schedule: job.schedule,
			action,
			overlap: job.overlap,
			...(job.lastRunMs === undefined
				? {}
				: { lastRun: new Date(job.lastRunMs) }),
			...(job.nextRunMs === undefined
				? {}
				: { nextRun: new Date(job.nextRunMs) }),
			runCount: job.runCount,
			running: job.running,
		};
	}

	private emit(event: SidecarCronEventRecord): void {
		let publicEvent: CronEvent;
		if (event.kind === "fire") {
			publicEvent = {
				type: "cron:fire",
				jobId: event.jobId,
				time: new Date(event.timeMs),
			};
		} else if (event.kind === "complete") {
			if (event.durationMs === undefined) {
				throw new Error("sidecar complete cron event is missing durationMs");
			}
			publicEvent = {
				type: "cron:complete",
				jobId: event.jobId,
				time: new Date(event.timeMs),
				durationMs: event.durationMs,
			};
		} else {
			if (event.error === undefined) {
				throw new Error("sidecar error cron event is missing error");
			}
			publicEvent = {
				type: "cron:error",
				jobId: event.jobId,
				time: new Date(event.timeMs),
				error: new Error(event.error),
			};
		}
		for (const listener of this.listeners) {
			try {
				listener(publicEvent);
			} catch (error) {
				console.warn("[agent-os] cron event listener failed", error);
			}
		}
	}

	private applyAlarm(alarm: SidecarCronAlarm): void {
		if (alarm.generation < this.alarmGeneration) return;
		this.alarmGeneration = alarm.generation;
		this.alarmDriver.set(alarm, (generation) => this.wake(generation));
	}

	private replaceJobCallback(
		jobId: string,
		callbackId: string | undefined,
	): void {
		const previous = this.callbackByJob.get(jobId);
		if (previous !== undefined && previous !== callbackId) {
			this.callbackByJob.delete(jobId);
			const route = this.callbacks.get(previous);
			if (route) route.scheduled = false;
			this.releaseCallback(previous);
		}
		if (callbackId !== undefined) {
			const route = this.callbacks.get(callbackId);
			if (!route)
				throw new Error(`cron callback route not found: ${callbackId}`);
			route.scheduled = true;
			this.callbackByJob.set(jobId, callbackId);
		}
	}

	private releaseCallback(callbackId: string): void {
		const route = this.callbacks.get(callbackId);
		if (route && !route.scheduled && route.activeRuns === 0) {
			this.callbacks.delete(callbackId);
		}
	}

	private allocateCallbackId(): string {
		this.callbackSequence++;
		if (!Number.isSafeInteger(this.callbackSequence)) {
			throw new Error("cron callback id counter exhausted; recreate the VM");
		}
		return `host-cron-callback-${this.callbackSequence}`;
	}

	private ensureActive(): void {
		if (this.disposed) throw new Error("cron manager is disposed");
	}
}

function decodeWireAction(value: unknown): WireCronAction {
	if (!value || typeof value !== "object") {
		throw new TypeError("sidecar returned an invalid cron action");
	}
	const action = value as Record<string, unknown>;
	if (action.type === "callback" && typeof action.callbackId === "string") {
		return { type: "callback", callbackId: action.callbackId };
	}
	if (
		action.type === "exec" &&
		typeof action.command === "string" &&
		(action.args === undefined ||
			(Array.isArray(action.args) &&
				action.args.every((arg) => typeof arg === "string")))
	) {
		return {
			type: "exec",
			command: action.command,
			...(action.args === undefined ? {} : { args: action.args as string[] }),
		};
	}
	if (
		action.type === "session" &&
		typeof action.agentType === "string" &&
		typeof action.prompt === "string"
	) {
		return {
			type: "session",
			agentType: action.agentType as Exclude<
				CronAction,
				{ type: "callback" | "exec" }
			>["agentType"],
			prompt: action.prompt,
			...(action.options === undefined
				? {}
				: {
						options: action.options as Exclude<
							CronAction,
							{ type: "callback" | "exec" }
						>["options"],
					}),
		};
	}
	throw new TypeError("sidecar returned an invalid cron action");
}

function normalizeScheduleError(schedule: string, error: unknown): unknown {
	const message = error instanceof Error ? error.message : String(error);
	if (message.includes("[invalid_schedule]"))
		return new InvalidScheduleError(schedule);
	if (message.includes("[past_schedule]"))
		return new PastScheduleError(schedule);
	return error;
}

async function missingCallback(): Promise<never> {
	throw new Error("cron callback route is unavailable");
}
