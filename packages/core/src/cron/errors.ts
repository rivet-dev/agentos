export class InvalidScheduleError extends Error {
	readonly schedule: string;

	constructor(schedule: string) {
		super(
			`Invalid schedule "${schedule}". Expected a cron expression or an ISO-like one-shot timestamp.`,
		);
		this.name = "InvalidScheduleError";
		this.schedule = schedule;
	}
}

export class PastScheduleError extends Error {
	readonly schedule: string;

	constructor(schedule: string) {
		super(`One-shot schedule "${schedule}" is already in the past.`);
		this.name = "PastScheduleError";
		this.schedule = schedule;
	}
}
