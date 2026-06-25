import { afterEach, describe, expect, test } from "vitest";
import {
	createInMemoryFileSystem,
	createKernel,
	type Kernel,
} from "../src/runtime-compat.js";

describe("runtime-compat mountFs bookkeeping", () => {
	let kernel: Kernel | undefined;

	afterEach(async () => {
		await kernel?.dispose();
		kernel = undefined;
	});

	test("unmountFs cancels a queued mount before kernel initialization", async () => {
		const mounted = createInMemoryFileSystem();
		await mounted.writeFile("/file.txt", "should not be visible");

		kernel = createKernel({
			filesystem: createInMemoryFileSystem(),
		});
		kernel.mountFs("/queued", mounted);
		kernel.unmountFs("/queued");

		await expect(kernel.readFile("/queued/file.txt")).rejects.toThrow();
	});
});
