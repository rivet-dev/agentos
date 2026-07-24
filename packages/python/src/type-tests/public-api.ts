import type { CodeEvaluationResult, CodeExecutionResult } from "../index.js";
import { PythonRuntime } from "../index.js";

async function publicApiContract(): Promise<void> {
	const runtime = await PythonRuntime.create();
	const execution: CodeExecutionResult =
		await runtime.execute("print('hello')");
	const evaluation: CodeEvaluationResult<number> =
		await runtime.evaluate<number>("21 * 2");
	await runtime.executeModule("json.tool", { args: [], stdin: "{}" });
	await runtime.install(["requests"]);
	void execution;
	void evaluation;
	await runtime.dispose();
}

void publicApiContract;
