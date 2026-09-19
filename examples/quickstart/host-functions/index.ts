import { AgentOs, hostFunction, hostFunctions } from "@rivet-dev/agentos-core";
import { z } from "zod";

const weatherFunctions = hostFunctions({
	name: "weather",
	description: "Look up weather information for cities.",
	functions: {
		get: hostFunction({
			description: "Get the current weather for a city.",
			inputSchema: z.object({
				city: z.string().describe("City name (e.g. 'London')."),
			}),
			execute: async ({ city }) => ({
				city,
				temperature: 18,
				conditions: "partly cloudy",
				humidity: 65,
			}),
			examples: [
				{ description: "Get London weather", input: { city: "London" } },
			],
		}),
	},
});

const calculatorFunctions = hostFunctions({
	name: "calc",
	description: "Simple calculator operations.",
	functions: {
		add: hostFunction({
			description: "Add two numbers.",
			inputSchema: z.object({ a: z.number(), b: z.number() }),
			execute: ({ a, b }) => ({ result: a + b }),
		}),
	},
});

const vm = await AgentOs.create({
	hostFunctions: [weatherFunctions, calculatorFunctions],
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
