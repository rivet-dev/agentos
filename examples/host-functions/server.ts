import { agentOS, setup } from "@rivet-dev/agentos";
import { z } from "zod";

// Host functions are a record of collections. The keys name everything: the
// collection key becomes the CLI command /bin/agentos-{name} inside the VM, and
// each function key becomes one of its subcommands. A function needs only a Zod
// input schema and an `execute` handler that runs on the host; `.describe()` on
// the schema is what the agent reads.
const vm = agentOS({
	hostFunctions: {
		weather: {
			forecast: {
				inputSchema: z
					.object({
						city: z.string().describe("City name"),
						days: z.number().optional().describe("Number of days"),
					})
					.describe("Get the weather forecast for a city"),
				execute: async ({ city, days }) => {
					const res = await fetch(
						`https://api.weather.example/forecast?city=${city}&days=${days ?? 3}`,
					);
					return res.json();
				},
				examples: [
					{
						description: "3-day forecast for Paris",
						input: { city: "Paris", days: 3 },
					},
				],
			},
		},
	},
});

export const registry = setup({ use: { vm } });
registry.start();
