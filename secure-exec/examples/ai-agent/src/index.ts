import { check, evaluate } from "secure-exec/typescript";

// docs:start tool
interface RunCodeResult {
	ok: boolean;
	value?: unknown;
	/** Type errors or a runtime failure for the model to fix and retry. */
	feedback?: string;
}

// The handler for a `run_code` tool. The source comes from a model, so it is
// never trusted: it runs in a fresh VM with no network and a hard timeout.
async function runCode(
	source: string,
	inputs: Record<string, number[]>,
): Promise<RunCodeResult> {
	// Type-check first. Diagnostics are cheap feedback that saves a run.
	const checked = await check(
		`declare const inputs: { prices: number[] };\n${source}`,
	);
	if (checked.outcome !== "succeeded" || checked.hasErrors) {
		const feedback = checked.diagnostics
			.map((diagnostic) => `TS${diagnostic.code}: ${diagnostic.message}`)
			.join("\n");
		return { ok: false, feedback };
	}

	const result = await evaluate(source, {
		inputs,
		timeoutMs: 5_000,
		output: { capture: "stderr" },
	});
	if (result.outcome !== "succeeded") {
		return { ok: false, feedback: result.stderr ?? result.error.message };
	}
	return { ok: true, value: result.value };
}
// docs:end tool

// docs:start loop
// The model's first attempt misnames a field, so it gets the diagnostics back
// without the code ever running.
const first = await runCode(
	`inputs.price.reduce((sum, price) => sum + price, 0)`,
	{ prices: [5, 10, 27] },
);
console.log(first); // { ok: false, feedback: "TS2551: Property 'price' does not exist ..." }

// The corrected attempt runs.
const second = await runCode(
	`inputs.prices.reduce((sum, price) => sum + price, 0)`,
	{ prices: [5, 10, 27] },
);
console.log(second); // { ok: true, value: 42 }
// docs:end loop
