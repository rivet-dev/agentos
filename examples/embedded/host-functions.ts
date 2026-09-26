import { AgentOs } from "@rivet-dev/agentos-core";
import { z } from "zod";

// Host functions are embedded-only. Pass them to AgentOs.create() and
// `execute` runs in this trusted host process, with schema-typed input.
const vm = await AgentOs.create({
	hostFunctions: {
		weather: {
			forecast: {
				inputSchema: z
					.object({ city: z.string().describe("City name") })
					.describe("Get the weather forecast for a city"),
				execute: async ({ city }) => ({ city, temperature: 22 }),
			},
		},
	},
});

// The agent calls it as `agentos-weather forecast --city Paris`.
const result = await vm.process.exec("agentos-weather forecast --city Paris", {
	output: { capture: "all" },
});
console.log(result.stdout);
await vm.dispose();
