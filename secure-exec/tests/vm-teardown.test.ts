import { afterEach, expect, test } from "vitest";
import { z } from "zod";
import { createVm, shutdown } from "../src/index.js";

// Keep the real callback-to-disposal path covered independently of the
// one-shot facade, so callback completion is asserted before teardown.
afterEach(shutdown);

test("completed host callbacks do not keep VM teardown alive", async () => {
	let calls = 0;
	const vm = await createVm({
		defaultSoftware: false,
		hostFunctions: {
			math: {
				add: {
					inputSchema: z.object({ a: z.number(), b: z.number() }),
					execute: ({ a, b }) => {
						calls++;
						return a + b;
					},
				},
			},
		},
	});
	try {
		const result = await vm.javascript.evaluate("math.add({ a: 40, b: 2 })");
		expect(result).toMatchObject({ outcome: "succeeded", value: 42 });
		expect(calls).toBe(1);
	} finally {
		await vm.dispose();
	}
});
