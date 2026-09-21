import { AgentOs } from "@rivet-dev/agentos-core";
import { z } from "zod";

const vm = await AgentOs.create({
	hostFunctions: {
		weather: {
			get: {
				inputSchema: z
					.object({
						city: z.string().describe("City name (e.g. 'London')."),
					})
					.describe("Get the current weather for a city."),
				execute: async ({ city }) => ({
					city,
					temperature: 18,
					conditions: "partly cloudy",
					humidity: 65,
				}),
				examples: [
					{ description: "Get London weather", input: { city: "London" } },
				],
			},
		},
		calc: {
			add: {
				inputSchema: z
					.object({ a: z.number(), b: z.number() })
					.describe("Add two numbers."),
				execute: ({ a, b }) => ({ result: a + b }),
			},
		},
	},
	permissions: {
		fs: "allow",
		network: "allow",
		childProcess: "allow",
		env: "allow",
		hostFunction: "allow",
	},
});

try {
	const weather = await vm.process.exec("agentos-weather get --city London", {
		output: { capture: "all" },
	});
	console.log("Weather:", (weather.stdout ?? "").trim());

	const sum = await vm.process.exec("agentos-calc add --a 10 --b 32", {
		output: { capture: "all" },
	});
	console.log("Sum:", (sum.stdout ?? "").trim());
} finally {
	await vm.dispose();
}
