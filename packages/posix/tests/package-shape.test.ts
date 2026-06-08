import { describe, expect, test } from "vitest";

describe("posix package shape", () => {
	test("reserved package export is importable and intentionally empty", async () => {
		const module = await import("../dist/index.js");

		expect(Object.keys(module)).toEqual([]);
	});
});
