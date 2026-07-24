import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const javascript = await JavaScriptRuntime.create();
try {
	await javascript.execute("const answer = 40", {
		executionId: "analysis",
		createIfMissing: true,
	});
	const result = await javascript.evaluate<number>("answer + inputs.increment", {
		executionId: "analysis",
		inputs: { increment: 2 },
	});
	if (result.outcome !== "succeeded") throw new Error(result.error.message);
	console.log(result.value); // 42

	await javascript.vm.executions.reset("analysis");
	await javascript.vm.executions.delete("analysis");
} finally {
	await javascript.dispose();
}
