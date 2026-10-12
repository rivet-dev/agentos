import { AgentOs } from "@rivet-dev/agentos";
import { z } from "zod";

// The handler runs on the host, so the API key never enters the VM.
const vm = await AgentOs.create({
	hostFunctions: {
		weather: {
			forecast: {
				inputSchema: z
					.object({ city: z.string() })
					.describe("Get the weather forecast for a city"),
				execute: async ({ city }) => {
					const res = await fetch(
						`https://api.weather.example/forecast?city=${city}&key=${process.env.WEATHER_API_KEY}`,
					);
					return res.json();
				},
			},
		},
	},
});

try {
	// The collection is projected into the VM as an `agentos-weather` command,
	// so it composes with pipes and redirects like any other program.
	const result = await vm.process.exec(
		"agentos-weather forecast --city Paris > /tmp/forecast.json && wc -c < /tmp/forecast.json",
		{ output: { capture: "all" } },
	);
	console.log(result.outcome === "succeeded" ? result.stdout : result.error);
} finally {
	await vm.dispose();
}
