import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CronManager } from "../src/cron/cron-manager.js";
import { InvalidScheduleError, PastScheduleError } from "../src/cron/errors.js";
import type { CronEvent } from "../src/cron/types.js";

const session = { connectionId: "connection", sessionId: "session" };
const sidecarVm = { vmId: "vm" };

class MockAlarmDriver {
	alarm: { generation: number; nextAlarmMs?: number } | undefined;
	wake: ((generation: number) => Promise<void>) | undefined;
	disposed = false;

	set(
		alarm: { generation: number; nextAlarmMs?: number },
		wake: (generation: number) => Promise<void>,
	): void {
		this.alarm = alarm;
		this.wake = wake;
	}

	async fire(): Promise<void> {
		if (!this.alarm || !this.wake) throw new Error("no cron alarm armed");
		await this.wake(this.alarm.generation);
	}

	dispose(): void {
		this.disposed = true;
	}
}

function createMockTransport() {
	return {
		scheduleCron: vi.fn().mockResolvedValue({
			id: "sidecar-id",
			alarm: { generation: 1, nextAlarmMs: Date.now() + 1_000 },
		}),
		listCronJobs: vi.fn().mockResolvedValue({
			jobs: [],
			alarm: { generation: 1, nextAlarmMs: Date.now() + 1_000 },
		}),
		cancelCronJob: vi.fn().mockResolvedValue({
			id: "sidecar-id",
			cancelled: true,
			alarm: { generation: 2 },
		}),
		wakeCron: vi.fn().mockResolvedValue({
			alarm: { generation: 2 },
			runs: [],
			events: [],
		}),
		completeCronRun: vi.fn().mockResolvedValue({
			alarm: { generation: 2 },
			runs: [],
			events: [],
		}),
	};
}

describe("CronManager thin host adapter", () => {
	let transport: ReturnType<typeof createMockTransport>;
	let alarmDriver: MockAlarmDriver;
	let manager: CronManager;

	beforeEach(() => {
		transport = createMockTransport();
		alarmDriver = new MockAlarmDriver();
		manager = new CronManager(
			transport as never,
			session,
			sidecarVm,
			alarmDriver,
		);
	});

	afterEach(() => manager.dispose());

	it("forwards only caller-supplied schedule fields and uses the sidecar ID", async () => {
		const job = await manager.schedule({
			schedule: "* * * * *",
			action: { type: "exec", command: "echo", args: ["hello"] },
		});

		expect(job.id).toBe("sidecar-id");
		expect(transport.scheduleCron).toHaveBeenCalledWith(session, sidecarVm, {
			schedule: "* * * * *",
			action: { type: "exec", command: "echo", args: ["hello"] },
		});
		expect(alarmDriver.alarm?.generation).toBe(1);
	});

	it("keeps callback functions host-side and sends only a correlation ID", async () => {
		const fn = vi.fn();
		await manager.schedule({
			id: "callback-job",
			schedule: "* * * * *",
			action: { type: "callback", fn },
			overlap: "skip",
		});

		const wireOptions = transport.scheduleCron.mock.calls[0][2];
		expect(wireOptions).toMatchObject({
			id: "callback-job",
			schedule: "* * * * *",
			overlap: "skip",
			action: { type: "callback" },
		});
		expect(wireOptions.action).not.toHaveProperty("fn");

		transport.wakeCron.mockResolvedValueOnce({
			alarm: { generation: 2 },
			events: [{ kind: "fire", jobId: "callback-job", timeMs: 100 }],
			runs: [
				{
					runId: "run-1",
					jobId: "callback-job",
					action: wireOptions.action,
				},
			],
		});
		await alarmDriver.fire();
		await vi.waitFor(() => expect(fn).toHaveBeenCalledOnce());
		expect(transport.completeCronRun).toHaveBeenCalledWith(
			session,
			sidecarVm,
			"run-1",
			undefined,
		);
	});

	it("never executes a serializable action returned by a malformed sidecar", async () => {
		await manager.schedule({
			id: "exec-job",
			schedule: "* * * * *",
			action: { type: "exec", command: "printenv", args: ["$(id)", "a b"] },
		});
		transport.wakeCron.mockResolvedValueOnce({
			alarm: { generation: 2 },
			events: [],
			runs: [
				{
					runId: "run-exec",
					jobId: "exec-job",
					action: {
						type: "exec",
						command: "printenv",
						args: ["$(id)", "a b"],
					},
				},
			],
		});
		await alarmDriver.fire();
		await vi.waitFor(() =>
			expect(transport.completeCronRun).toHaveBeenCalledWith(
				session,
				sidecarVm,
				"run-exec",
				"sidecar returned non-host cron action to client: exec",
			),
		);
	});

	it("maps sidecar job state and events to the public API", async () => {
		const events: CronEvent[] = [];
		manager.onEvent((event) => events.push(event));
		transport.listCronJobs.mockResolvedValueOnce({
			alarm: { generation: 3 },
			jobs: [
				{
					id: "listed",
					schedule: "*/5 * * * *",
					action: { type: "exec", command: "true" },
					overlap: "queue",
					lastRunMs: 100,
					nextRunMs: 200,
					runCount: 2,
					running: true,
				},
			],
		});
		const jobs = await manager.list();
		expect(jobs[0]).toMatchObject({
			id: "listed",
			overlap: "queue",
			runCount: 2,
			running: true,
		});
		expect(jobs[0].lastRun?.getTime()).toBe(100);
		expect(jobs[0].nextRun?.getTime()).toBe(200);

		transport.wakeCron.mockResolvedValueOnce({
			alarm: { generation: 4 },
			runs: [],
			events: [{ kind: "error", jobId: "listed", timeMs: 300, error: "boom" }],
		});
		await alarmDriver.fire();
		expect(events[0]).toMatchObject({ type: "cron:error", jobId: "listed" });
	});

	it("rejects malformed cron events instead of inventing results", async () => {
		await manager.list();
		transport.wakeCron.mockResolvedValueOnce({
			alarm: { generation: 4 },
			runs: [],
			events: [{ kind: "complete", jobId: "listed", timeMs: 300 }],
		});
		await expect(alarmDriver.fire()).rejects.toThrow("missing durationMs");

		transport.wakeCron.mockResolvedValueOnce({
			alarm: { generation: 5 },
			runs: [],
			events: [{ kind: "error", jobId: "listed", timeMs: 301 }],
		});
		await expect(alarmDriver.fire()).rejects.toThrow("missing error");
	});

	it("maps sidecar schedule rejection markers to public error classes", async () => {
		transport.scheduleCron.mockRejectedValueOnce(
			new Error("[invalid_schedule] malformed"),
		);
		await expect(
			manager.schedule({
				schedule: "tomorrow",
				action: { type: "exec", command: "true" },
			}),
		).rejects.toBeInstanceOf(InvalidScheduleError);

		transport.scheduleCron.mockRejectedValueOnce(
			new Error("[past_schedule] past"),
		);
		await expect(
			manager.schedule({
				schedule: "2020-01-01T00:00:00Z",
				action: { type: "exec", command: "true" },
			}),
		).rejects.toBeInstanceOf(PastScheduleError);
	});

	it("forwards cancellation and disposes only its host alarm", async () => {
		const job = await manager.schedule({
			schedule: "* * * * *",
			action: { type: "exec", command: "true" },
		});
		await job.cancel();
		expect(transport.cancelCronJob).toHaveBeenCalledWith(
			session,
			sidecarVm,
			"sidecar-id",
		);
		manager.dispose();
		expect(alarmDriver.disposed).toBe(true);
	});
});
