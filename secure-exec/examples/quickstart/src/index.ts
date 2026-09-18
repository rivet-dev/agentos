// docs:start evaluate
import { evaluate, execute } from "secure-exec";

// Each call runs in a fresh VM that is disposed when the call finishes.
const sum = await evaluate<number>("1 + 2");
console.log(sum.outcome === "succeeded" ? sum.value : sum.error); // 3
// docs:end evaluate

// docs:start inputs
// `inputs` hands host values to the code as real objects, so data is never
// interpolated into source.
const total = await evaluate<number>(
	"inputs.prices.reduce((a, b) => a + b, 0)",
	{ inputs: { prices: [5, 10, 27] } },
);
console.log(total.outcome === "succeeded" ? total.value : total.error); // 42
// docs:end inputs

// docs:start statements
// `evaluate` takes one expression, so several statements go in a function.
const report = await evaluate<{ count: number; max: number }>(`
	(() => {
		const values = [3, 9, 4];
		return { count: values.length, max: Math.max(...values) };
	})()
`);
console.log(report.outcome === "succeeded" ? report.value : report.error);
// docs:end statements

// docs:start execute
// `execute` runs a whole ES module for its side effects. Capture output to read
// it back.
const run = await execute(
	`
	import { platform } from "node:os";
	console.log("running on", platform());
	`,
	{ output: { capture: "all" } },
);
console.log(run.stdout?.trim()); // running on linux
// docs:end execute

// docs:start errors
// Guest errors are returned, not thrown. Capture stderr to see the stack.
const failed = await evaluate(`JSON.parse("not json")`, {
	output: { capture: "stderr" },
});
if (failed.outcome !== "succeeded") {
	console.log(failed.outcome, failed.stderr?.split("\n")[0]); // failed SyntaxError: ...
}
// docs:end errors
