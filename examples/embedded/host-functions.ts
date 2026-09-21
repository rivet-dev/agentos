import { AgentOs } from "@rivet-dev/agentos-core";
import { z } from "zod";

// Host functions are defined exactly as they are for the actor. Pass them to
// AgentOs.create() and `execute` runs in this host process, with its input typed
// by its own schema.
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
const result = await vm.process.exec("agentos-weather forecast --city Paris");
console.log(result.stdout);
await vm.dispose();
