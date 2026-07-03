// Bindings: define functions that execute on the host and are callable
// from inside the VM as CLI commands.

import { AgentOs, binding, bindingGroup } from "@rivet-dev/agentos-core";
import { z } from "zod";

const weatherBindings = bindingGroup({
	name: "weather",
	description: "Look up weather information for cities.",
	bindings: {
		get: binding({
			description: "Get the current weather for a city.",
			inputSchema: z.object({
				city: z.string().describe("City name (e.g. 'London')."),
			}),
			execute: async (input) => {
				const { city } = input;
				return {
					city,
					temperature: 18,
					conditions: "partly cloudy",
					humidity: 65,
				};
			},
			examples: [
				{ description: "Get London weather", input: { city: "London" } },
			],
		}),
	},
});

const calcBindings = bindingGroup({
	name: "calc",
	description: "Simple calculator operations.",
	bindings: {
		add: binding({
			description: "Add two numbers.",
			inputSchema: z.object({ a: z.number(), b: z.number() }),
			execute: (input) => ({ result: input.a + input.b }),
		}),
	},
});

const vm = await AgentOs.create({
	bindings: [weatherBindings, calcBindings],
	permissions: {
		fs: "allow",
		network: "allow",
		childProcess: "allow",
		env: "allow",
		binding: "allow",
	},
});

const weather = await vm.exec("agentos-weather get --city London");
console.log("Weather:", JSON.stringify(weather));

const sum = await vm.exec("agentos-calc add --a 10 --b 32");
console.log("Sum:", JSON.stringify(sum));

await vm.dispose();
