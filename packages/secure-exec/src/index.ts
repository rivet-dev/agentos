import type {
	CodeEvaluationResult,
	CodeExecutionResult,
	JavaScriptEvaluationOptions,
	JavaScriptExecutionOptions,
	JsonValue,
} from "@rivet-dev/agentos-core";
import { run, type Target } from "./runtime.js";

export type {
	CodeEvaluationResult,
	CodeExecutionResult,
	ExecutionErrorData,
	ExecutionOutputOptions,
	JsonValue,
	LimitWarning,
	MountConfig,
	OutputCapture,
	Permissions,
} from "@rivet-dev/agentos-core";
export {
	createHostDirBackend,
	nodeModulesMount,
} from "@rivet-dev/agentos-core";
export {
	type Context,
	createContext,
	init,
	type Target,
	type VmOptions,
} from "./runtime.js";

export type ExecuteOptions = Omit<JavaScriptExecutionOptions, "contextId"> &
	Target;
export type EvaluateOptions = Omit<JavaScriptEvaluationOptions, "contextId"> &
	Target;

/** Run JavaScript for its side effects and captured output. */
export function execute(
	source: string,
	options?: ExecuteOptions,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.javascript.execute(source, operationOptions),
	);
}

/** Evaluate one JavaScript expression and return its JSON value. */
export function evaluate<T = JsonValue>(
	source: string,
	options?: EvaluateOptions,
): Promise<CodeEvaluationResult<T>> {
	return run(options, (vm, operationOptions) =>
		vm.javascript.evaluate<T>(source, operationOptions),
	);
}
