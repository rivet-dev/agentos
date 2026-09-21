// docs:start hostFunctions
import { AgentOs } from "@rivet-dev/agentos-core";
import { z } from "zod";

const runtime = await AgentOs.create({
	hostFunctions: {
		tools: {
			weather: {
				inputSchema: z
					.object({ city: z.string() })
					.describe("Look up a city's temperature."),
				execute: ({ city }) => ({
					city,
					tempF: city === "San Francisco" ? 61 : 75,
				}),
			},
		},
	},
});
// docs:end hostFunctions

// docs:start generated-code
// Each collection is a global inside the VM, and each host function is an
// async function, so generated code calls your tools like any other API.
const llmGeneratedExpression = `(async () => {
  const [sf, tokyo] = await Promise.all([
    tools.weather({ city: "San Francisco" }),
    tools.weather({ city: "Tokyo" }),
  ]);
  return {
    sanFrancisco: sf,
    tokyo,
    differenceF: Math.abs(sf.tempF - tokyo.tempF),
  };
})()`;
// docs:end generated-code

// docs:start run
try {
	const result = await runtime.javascript.evaluate<{
		sanFrancisco: { city: string; tempF: number };
		tokyo: { city: string; tempF: number };
		differenceF: number;
	}>(llmGeneratedExpression, { timeoutMs: 5_000 });
	console.log(result);
} finally {
	await runtime.dispose();
}
// docs:end run
