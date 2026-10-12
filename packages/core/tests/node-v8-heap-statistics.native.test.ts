import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const source = readFileSync(
	new URL("./fixtures/v8-heap-statistics.mjs", import.meta.url),
	"utf8",
);
const fields = [
	"does_zap_garbage",
	"external_memory",
	"heap_size_limit",
	"malloced_memory",
	"number_of_detached_contexts",
	"number_of_native_contexts",
	"peak_malloced_memory",
	"total_available_size",
	"total_global_handles_size",
	"total_heap_size",
	"total_heap_size_executable",
	"total_physical_size",
	"used_global_handles_size",
	"used_heap_size",
];

test("node:v8 heap statistics report live isolate use through CJS and ESM", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	const errors: unknown[] = [];
	try {
		vm = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
			allowedNodeBuiltins: ["module", "v8", "buffer"],
		});
		await vm.filesystem.writeFile("/tmp/v8-heap-statistics.mjs", source);
		const result = await vm.process.execFile(
			"node",
			["/tmp/v8-heap-statistics.mjs"],
			{
				timeoutMs: 3_000,
				output: { capture: "all" },
			},
		);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stderr).toBe("");
		expect(Buffer.byteLength(result.stdout)).toBeLessThanOrEqual(16_384);
		const actual = JSON.parse(result.stdout);
		console.info("V8_LIVE_HEAP_STATISTICS", JSON.stringify(actual));
		for (const sample of [actual.before, actual.after]) {
			expect(Object.keys(sample).sort()).toEqual(fields);
			for (const value of Object.values(sample)) {
				expect(typeof value).toBe("number");
				expect(Number.isSafeInteger(value)).toBe(true);
				expect(value).toBeGreaterThanOrEqual(0);
			}
			expect(sample.heap_size_limit).toBeGreaterThan(0);
			expect(sample.used_heap_size).toBeLessThanOrEqual(sample.total_heap_size);
			expect(sample.used_global_handles_size).toBeLessThanOrEqual(
				sample.total_global_handles_size,
			);
		}
		expect(actual.retainedCount).toBe(25_000);
		expect(actual.sameBinding).toBe(true);
		// Reuse the existing process_runtime_stats_are_live bounded allocation check.
		expect(actual.after.used_heap_size).toBeGreaterThan(
			actual.before.used_heap_size + 512_000,
		);
	} catch (error) {
		errors.push(error);
	} finally {
		try {
			await vm?.dispose();
		} catch (error) {
			errors.push(error);
		}
		try {
			await sidecar.dispose();
		} catch (error) {
			errors.push(error);
		}
	}
	if (errors.length)
		throw new AggregateError(errors, "live V8 heap statistics failed");
}, 15_000);
