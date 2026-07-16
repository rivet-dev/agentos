import { describe, expect, test } from "vitest";
import {
	NodeRuntime,
	nodeRuntimeCreateOptionsSchema,
} from "../src/index.js";
import { createInMemoryFileSystem } from "../src/test-runtime.js";

describe("NodeRuntime create options validation", () => {
	test("rejects unknown top-level options before booting a VM", async () => {
		await expect(
			NodeRuntime.create({
				filesystem: createInMemoryFileSystem(),
				notARealOption: true,
			} as never),
		).rejects.toThrow(/notARealOption/);
	});

	test("rejects unknown nested permission fields", () => {
		expect(() =>
			nodeRuntimeCreateOptionsSchema.parse({
				filesystem: createInMemoryFileSystem(),
				permissions: {
					filesystem: "allow",
				},
			}),
		).toThrow(/filesystem/);
	});
});
