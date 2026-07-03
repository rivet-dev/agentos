import { agentOS, setup } from "@rivet-dev/agentos";
import { z } from "zod";

// Define a group of bindings (host functions). Each binding has a Zod input
// schema and an `execute` handler that runs on the host. The group is exposed to
// the agent as a CLI command at /usr/local/bin/agentos-{name} inside the VM.
const weatherBindings = {
  name: "weather",
  description: "Weather data bindings",
  bindings: {
    forecast: {
      description: "Get the weather forecast for a city",
      inputSchema: z.object({
        city: z.string().describe("City name"),
        days: z.number().optional().describe("Number of days"),
      }),
      execute: async (input: { city: string; days?: number }) => {
        const res = await fetch(
          `https://api.weather.example/forecast?city=${input.city}&days=${input.days ?? 3}`,
        );
        return res.json();
      },
      examples: [
        { description: "3-day forecast for Paris", input: { city: "Paris", days: 3 } },
      ],
    },
  },
};

const vm = agentOS({
  bindings: [weatherBindings],
});

export const registry = setup({ use: { vm } });
registry.start();
