import { describe, expect, test } from "vitest";
import {
	NodeRuntime,
	nodeRuntimeCreateOptionsSchema,
} from "../src/index.js";

describe("NodeRuntime create options validation", () => {
	test("rejects unknown top-level options before booting a VM", async () => {
		await expect(
			NodeRuntime.create({
				notARealOption: true,
			} as never),
		).rejects.toThrow(/notARealOption/);
	});

	test("rejects unknown nested permission fields", () => {
		expect(() =>
			nodeRuntimeCreateOptionsSchema.parse({
				permissions: {
					filesystem: "allow",
				},
			}),
		).toThrow(/filesystem/);
	});
});
