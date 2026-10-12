import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const source = readFileSync(
	new URL("./fixtures/v8-json-serialization.mjs", import.meta.url),
	"utf8",
);

test("node:v8 CJS and ESM reject lossy values and preserve JSON-compatible data", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		allowedNodeBuiltins: ["module", "v8", "buffer"],
	});
	const errors: unknown[] = [];
	try {
		await vm.filesystem.writeFile("/tmp/v8-json-serialization.mjs", source);
		const result = await vm.process.execFile(
			"node",
			["/tmp/v8-json-serialization.mjs"],
			{
				timeoutMs: 3_000,
				output: { capture: "all" },
			},
		);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stderr).toBe("");
		expect(Buffer.byteLength(result.stdout)).toBeLessThanOrEqual(16_384);
		const actual = JSON.parse(result.stdout);
		console.info("V8_JSON_SERIALIZATION", JSON.stringify(actual));
		expect(actual.sameBindings).toBe(true);
		expect(actual.sameDefault).toBe(true);
		expect(actual.exportNames).toEqual([
			"DefaultDeserializer",
			"DefaultSerializer",
			"Deserializer",
			"GCProfiler",
			"Serializer",
			"cachedDataVersionTag",
			"deserialize",
			"getCppHeapStatistics",
			"getHeapCodeStatistics",
			"getHeapSnapshot",
			"getHeapSpaceStatistics",
			"getHeapStatistics",
			"isStringOneByteRepresentation",
			"promiseHooks",
			"queryObjects",
			"serialize",
			"setFlagsFromString",
			"setHeapSnapshotNearHeapLimit",
			"startCpuProfile",
			"startupSnapshot",
			"stopCoverage",
			"takeCoverage",
			"writeHeapSnapshot",
		]);
		expect(actual.esmExportNames).toEqual(actual.exportNames);
		expect(actual.accessorCalls).toBe(0);
		expect(actual.toJsonCalls).toBe(0);
		for (const api of [actual.cjs, actual.esm]) {
			expect(Object.keys(api.rejected)).toHaveLength(30);
			for (const code of Object.values(api.rejected))
				expect(code).toBe("ERR_NOT_IMPLEMENTED");
			expect(api.invalid).toBe("ERR_NOT_IMPLEMENTED");
			expect(api.roundTrips).toEqual([
				null,
				"",
				false,
				0,
				1.5,
				{ s: 'x\n"', n: 1.5, b: true, z: null, list: [1, { nested: "y" }] },
				{ value: 3 },
			]);
			expect(api.classRoundTrip).toEqual(api.roundTrips[5]);
			expect(api.arrayMutationRoundTrip).toEqual([{ value: 1 }, { kept: 2 }]);
		}
	} catch (error) {
		errors.push(error);
	} finally {
		try {
			await vm.dispose();
		} catch (error) {
			errors.push(error);
		}
	}
	if (errors.length)
		throw new AggregateError(errors, "v8 JSON serialization failed");
}, 15_000);
