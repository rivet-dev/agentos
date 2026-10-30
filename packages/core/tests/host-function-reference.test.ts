import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";
const mathFunctions = {
	add: {
		inputSchema: z
			.object({
				a: z.number(),
				b: z.number(),
			})
			.describe("Add two numbers"),
		execute: ({ a, b }) => ({ sum: a + b }),
		examples: [
			{
				description: "Add 1 and 2",
				input: { a: 1, b: 2 },
			},
		],
	},
};

describe("hostFunction reference registration", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			hostFunctions: { math: mathFunctions },
		});
	});

	afterEach(async () => {
		await vm.dispose();
	});

	test("stores generated hostFunction reference markdown on the VM", () => {
		const reference = (vm as unknown as { _hostFunctionReference: string })
			._hostFunctionReference;

		expect(reference).toContain("## Available Host Functions");
		expect(reference).toContain(
			"Run `agentos list-host-functions` to see all available host functions.",
		);
		expect(reference).toContain("### math");
		expect(reference).toContain(
			"`agentos-math add --a <number> --b <number>` - Add two numbers",
		);
		expect(reference).toContain("Add 1 and 2");
	});
});
