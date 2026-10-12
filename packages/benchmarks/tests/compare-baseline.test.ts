import assert from "node:assert/strict";
import { describe, test } from "node:test";
import type { MatrixBaseline, MatrixBaselineRow } from "../src/baseline.js";
import { compareMatrixBaseline } from "../src/compare-baseline.js";

const OPTIONS = {
	threshold: 2,
	tinyBaselineFloorMs: 0.5,
	tinyCurrentFloorMs: 1,
};

function row(p50Ms: number): MatrixBaselineRow {
	return {
		key: "fs/fs_read_small",
		family: "fs",
		op: "fs_read_small",
		lanes: { guest: { p50Ms } },
		tax: {},
	};
}

function compare(baselineP50Ms: number, currentP50Ms: number) {
	const baseline = {
		rows: [row(baselineP50Ms)],
	} as MatrixBaseline;
	return compareMatrixBaseline(
		[row(currentP50Ms)],
		baseline,
		[{ key: "fs/fs_read_small", lane: "guest" }],
		OPTIONS,
	)[0];
}

describe("compareMatrixBaseline", () => {
	test("rejects the same small-write slowdown as main", () => {
		const result = compare(0.31, 1.04);
		assert.equal(result.ratio, 3.35);
		assert.equal(result.status, "fail");
	});

	test("ignores current measurements below the tiny-row floor", () => {
		const result = compare(0.35, 0.95);
		assert.equal(result.ratio, 2.71);
		assert.equal(result.deltaMs, 0.6);
		assert.equal(result.status, "ignored");
	});

	test("fails at the tiny-row current-value boundary", () => {
		const result = compare(0.35, 1);
		assert.equal(result.ratio, 2.86);
		assert.equal(result.deltaMs, 0.65);
		assert.equal(result.status, "fail");
	});

	test("keeps ratio-only enforcement for normal baselines", () => {
		const result = compare(5, 10.1);
		assert.equal(result.ratio, 2.02);
		assert.equal(result.status, "fail");
	});
});
