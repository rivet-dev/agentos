import { PythonRuntime } from "@rivet-dev/agentos-python";

const python = await PythonRuntime.create();
try {
	await python.execute("answer = 40", {
		executionId: "analysis",
		createIfMissing: true,
	});
	const result = await python.evaluate<number>("answer + inputs['increment']", {
		executionId: "analysis",
		inputs: { increment: 2 },
	});
	if (result.outcome !== "succeeded") throw new Error(result.error.message);
	console.log(result.value); // 42

	await python.vm.executions.reset("analysis");
	await python.vm.executions.delete("analysis");
} finally {
	await python.dispose();
}
