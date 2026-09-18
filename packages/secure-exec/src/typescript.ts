import type {
	CodeEvaluationResult,
	CodeExecutionResult,
	JsonValue,
	TypeScriptCheckOptions,
	TypeScriptCheckResult,
	TypeScriptEvaluationOptions,
	TypeScriptExecutionOptions,
} from "@rivet-dev/agentos-core";
import { run, type Target } from "./runtime.js";

export type {
	TypeScriptCheckResult,
	TypeScriptDiagnostic,
} from "@rivet-dev/agentos-core";

export type ExecuteOptions = Omit<TypeScriptExecutionOptions, "contextId"> &
	Target;
export type EvaluateOptions = Omit<TypeScriptEvaluationOptions, "contextId"> &
	Target;
export type CheckOptions = Omit<TypeScriptCheckOptions, "contextId"> & Target;

/** Run TypeScript for its side effects and captured output. Types are stripped, not checked. */
export function execute(
	source: string,
	options?: ExecuteOptions,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.execute(source, operationOptions),
	);
}

/** Evaluate one TypeScript expression and return its JSON value. Types are stripped, not checked. */
export function evaluate<T = JsonValue>(
	source: string,
	options?: EvaluateOptions,
): Promise<CodeEvaluationResult<T>> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.evaluate<T>(source, operationOptions),
	);
}

/** Type-check TypeScript without running it. */
export function check(
	source: string,
	options?: CheckOptions,
): Promise<TypeScriptCheckResult> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.check(source, operationOptions),
	);
}
