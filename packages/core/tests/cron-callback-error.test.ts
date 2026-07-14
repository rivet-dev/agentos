import { afterEach, describe, expect, it, vi } from "vitest";
import type { CronEvent } from "../src/cron/types.js";
import { AgentOs } from "../src/index.js";

describe("cron callback failure integration", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
	});

	it("records a rejected host callback as a sidecar-owned error", async () => {
		vm = await AgentOs.create();
		const events: CronEvent[] = [];
		vm.onCronEvent((event) => events.push(event));
		const job = await vm.scheduleCron({
			id: "callback-failed",
			schedule: new Date(Date.now() + 500).toISOString(),
			action: {
				type: "callback",
				fn: async () => {
					throw new Error("typescript callback failed");
				},
			},
		});

		await vi.waitFor(
			() =>
				expect(
					events.some(
						(event) =>
							event.type === "cron:error" &&
							event.jobId === "callback-failed" &&
							event.error.message === "typescript callback failed",
					),
				).toBe(true),
			{ timeout: 5_000 },
		);

		const failedEvents = events.filter(
			(event) => event.jobId === "callback-failed",
		);
		expect(failedEvents.map((event) => event.type)).toEqual([
			"cron:fire",
			"cron:error",
		]);
		expect(
			failedEvents.some((event) => event.type === "cron:complete"),
		).toBe(false);
		const info = (await vm.listCronJobs()).find(
			(candidate) => candidate.id === "callback-failed",
		);
		expect(info).toMatchObject({ runCount: 1, running: false });
		await job.cancel();
	});
});
