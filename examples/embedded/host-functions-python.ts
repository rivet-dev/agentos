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
	// Python calls the `agentos-weather` command like any other program.
	const result = await vm.python.execute(
		`
import json
import subprocess

completed = subprocess.run(
    ["agentos-weather", "forecast", "--city", "Paris"],
    check=True,
    capture_output=True,
    text=True,
)
print(json.loads(completed.stdout)["result"])
`,
		{ output: { capture: "all" } },
	);
	console.log(result.outcome === "succeeded" ? result.stdout : result.error);
} finally {
	await vm.dispose();
}
