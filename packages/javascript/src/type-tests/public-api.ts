import type { CodeEvaluationResult, CodeExecutionResult } from "../index.js";
import { JavaScriptRuntime } from "../index.js";

async function publicApiContract(): Promise<void> {
	const runtime = await JavaScriptRuntime.create();
	const execution: CodeExecutionResult = await runtime.execute(
		"console.log('hello')",
	);
	const evaluation: CodeEvaluationResult<number> =
		await runtime.evaluate<number>("21 * 2");
	const check = await runtime.typescript.check("const answer: number = 42;");
	await runtime.typescript.execute("console.log('typed') satisfies string");
	const executionId = execution.executionId;
	await runtime.vm.executions.get(executionId);
	void execution;
	void evaluation;
	void check;
	await runtime.dispose();
}

void publicApiContract;
