import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CronEvent } from "../src/cron/types.js";
import { AgentOs } from "../src/index.js";
import { REGISTRY_SOFTWARE } from "./helpers/registry-commands.js";

describe("cron integration via AgentOs API", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({ software: REGISTRY_SOFTWARE });
	});

	afterEach(async () => {
		await vm.dispose();
	});

	it("schedules, lists, and cancels through the sidecar", async () => {
		const job = await vm.scheduleCron({
			id: "managed-by-sidecar",
			schedule: "*/5 * * * *",
			action: { type: "exec", command: "true" },
		});
		expect(job.id).toBe("managed-by-sidecar");

		const jobs = await vm.listCronJobs();
		expect(jobs).toHaveLength(1);
		expect(jobs[0]).toMatchObject({
			id: "managed-by-sidecar",
			schedule: "*/5 * * * *",
			overlap: "allow",
			runCount: 0,
			running: false,
		});

		await job.cancel();
		expect(await vm.listCronJobs()).toHaveLength(0);
	});

	it("routes a due callback and its lifecycle events through the protocol", async () => {
		const callback = vi.fn();
		const events: CronEvent[] = [];
		vm.onCronEvent((event) => events.push(event));

		await vm.scheduleCron({
			id: "callback-once",
			schedule: new Date(Date.now() + 500).toISOString(),
			action: { type: "callback", fn: callback },
		});

		await vi.waitFor(() => expect(callback).toHaveBeenCalledOnce(), {
			timeout: 5_000,
		});
		await vi.waitFor(
			() =>
				expect(events.some((event) => event.type === "cron:complete")).toBe(
					true,
				),
			{ timeout: 5_000 },
		);
		expect(events.map((event) => event.type)).toContain("cron:fire");
	});

	it("preserves exec argv without client-side shell evaluation", async () => {
		const events: CronEvent[] = [];
		vm.onCronEvent((event) => events.push(event));
		await vm.scheduleCron({
			id: "exec-once",
			schedule: new Date(Date.now() + 500).toISOString(),
			action: {
				type: "exec",
				command: "node",
				args: [
					"-e",
					"require('fs').writeFileSync('/tmp/cron-argv.json', JSON.stringify(process.argv.slice(1)))",
					"$(id)",
					"a b",
				],
			},
		});

		await vi.waitFor(
			async () => {
				const data = await vm.readFile("/tmp/cron-argv.json");
				expect(JSON.parse(new TextDecoder().decode(data))).toEqual([
					"$(id)",
					"a b",
				]);
			},
			{ timeout: 5_000 },
		);
		await vi.waitFor(
			() =>
				expect(
					events.some(
						(event) =>
							event.type === "cron:complete" && event.jobId === "exec-once",
					),
				).toBe(true),
			{ timeout: 5_000 },
		);
	});
});
