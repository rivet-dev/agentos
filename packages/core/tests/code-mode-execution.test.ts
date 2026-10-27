import {
	afterAll,
	beforeAll,
	beforeEach,
	describe,
	expect,
	test,
} from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

// Code mode: a script evaluated in the VM calls host functions, and the caller
// bounds it with timeouts, aborts, and memory limits.

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe("code mode execution", () => {
	let vm: AgentOs;
	let callStarts: number[] = [];
	let inFlight = 0;
	let maxInFlight = 0;
	const limitWarnings: string[] = [];

	beforeAll(async () => {
		vm = await AgentOs.create({
			permissions: ALLOW_ALL_VM_PERMISSIONS,
			onLimitWarning: (warning) => limitWarnings.push(warning.limit),
			hostFunctions: {
				tools: {
					wait: {
						inputSchema: z.object({ ms: z.number() }),
						execute: async ({ ms }: { ms: number }) => {
							callStarts.push(Date.now());
							inFlight += 1;
							maxInFlight = Math.max(maxInFlight, inFlight);
							await sleep(ms);
							inFlight -= 1;
							return {};
						},
					},
				},
			},
		});
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
	});

	beforeEach(() => {
		callStarts = [];
		maxInFlight = 0;
	});

	test("parallel host calls all run at once", async () => {
		const result = await vm.javascript.evaluate(
			"Promise.all(Array.from({ length: 20 }, () => tools.wait({ ms: 300 }))).then(() => true)",
			{ timeoutMs: 10_000 },
		);

		expect(result.outcome).toBe("succeeded");
		expect(maxInFlight).toBe(20);
	}, 30_000);

	test("a timed-out script starts no host calls after it ends", async () => {
		const result = await vm.javascript.evaluate(
			"Promise.all(Array.from({ length: 20 }, () => tools.wait({ ms: 1000 })))",
			{ timeoutMs: 300 },
		);
		const settledAt = Date.now();
		// Calls already running finish within their 1 s sleep.
		await sleep(1_500);

		expect(result.outcome).toBe("timed_out");
		expect(callStarts.filter((startedAt) => startedAt > settledAt)).toEqual([]);
	}, 30_000);

	test("aborting a busy script stops it without the cancel grace period", async () => {
		const controller = new AbortController();
		setTimeout(() => controller.abort(), 200);
		const startedAt = Date.now();
		const result = await vm.javascript.evaluate(
			"(() => { while (true) {} })()",
			{
				timeoutMs: 30_000,
				signal: controller.signal,
			},
		);

		expect(result.outcome).toBe("cancelled");
		// Cancellation falls back to a forced stop after a 1 s grace period.
		expect(Date.now() - startedAt).toBeLessThan(900);
		expect(await vm.javascript.evaluate("1 + 1")).toMatchObject({
			outcome: "succeeded",
			value: 2,
		});
	}, 30_000);

	test("a promise that never settles times out", async () => {
		const result = await vm.javascript.evaluate("new Promise(() => {})", {
			timeoutMs: 1_000,
		});

		expect(result.outcome).toBe("timed_out");
	}, 30_000);

	test("one allocation past the heap limit fails only its own script", async () => {
		const result = await vm.javascript.evaluate(
			"'a'.repeat(256 * 1024 * 1024).toUpperCase().length",
			{ timeoutMs: 10_000 },
		);

		expect(result).toMatchObject({
			outcome: "failed",
			error: {
				code: "execution_heap_limit_exceeded",
				message: expect.stringContaining("limits.jsRuntime.v8HeapLimitMb"),
			},
		});
		expect(await vm.javascript.evaluate("1 + 1")).toMatchObject({
			outcome: "succeeded",
			value: 2,
		});
	}, 30_000);

	test("buffer memory past the heap limit fails the allocation, not the VM", async () => {
		// 1 GiB of typed arrays against the default 128 MiB limit.
		const result = await vm.javascript.evaluate(
			`(() => {
				const held = [];
				try {
					for (let i = 0; i < 16; i++) held.push(new Uint8Array(64 * 1024 * 1024).fill(1));
				} catch (error) {
					return \`\${error.name}: \${error.message}\`;
				}
				return "held 1 GiB";
			})()`,
			{ timeoutMs: 10_000 },
		);

		expect(result).toMatchObject({
			outcome: "succeeded",
			value: "RangeError: Array buffer allocation failed",
		});
		await expect.poll(() => limitWarnings).toContain("v8_array_buffer_bytes");
		// The script's buffers are released, so the same VM can allocate again.
		expect(
			await vm.javascript.evaluate(
				"new Uint8Array(64 * 1024 * 1024).fill(1).length",
			),
		).toMatchObject({ outcome: "succeeded", value: 64 * 1024 * 1024 });
	}, 30_000);

	test("reading a file with no buffer memory left fails the read, not the sidecar", async () => {
		await vm.writeFile(
			"/tmp/host-bytes.bin",
			new Uint8Array(1024 * 1024).fill(7),
		);

		const result = await vm.javascript.evaluate(
			`(() => {
				const held = [];
				for (let size = 16 * 1024 * 1024; size >= 256; size >>= 1) {
					try {
						for (;;) held.push(new Uint8Array(size));
					} catch {}
				}
				try {
					return require("node:fs").readFileSync("/tmp/host-bytes.bin").length;
				} catch (error) {
					return \`\${error.name}: \${error.message}\`;
				}
			})()`,
			{ timeoutMs: 20_000 },
		);

		expect(result).toMatchObject({
			outcome: "succeeded",
			value: "RangeError: Array buffer allocation failed",
		});
		expect(await vm.javascript.evaluate("1 + 1")).toMatchObject({
			outcome: "succeeded",
			value: 2,
		});
	}, 30_000);
});
