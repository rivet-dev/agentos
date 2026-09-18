import type {
	CodeEvaluationResult,
	CodeExecutionResult,
	JsonValue,
	TypeScriptCheckOptions,
	TypeScriptCheckResult,
	TypeScriptEvaluationOptions,
	TypeScriptExecutionOptions,
	TypeScriptFileExecutionOptions,
} from "@rivet-dev/agentos-core";
import { type OneShot, run } from "./runtime.js";

export type {
	TypeScriptCheckResult,
	TypeScriptDiagnostic,
} from "@rivet-dev/agentos-core";

// One-shot conveniences, like the main entry point. On a VM from `createVm()`,
// use `vm.typescript`.

export type ExecuteOptions = OneShot<TypeScriptExecutionOptions>;
export type EvaluateOptions = OneShot<TypeScriptEvaluationOptions>;
export type ExecuteFileOptions = OneShot<TypeScriptFileExecutionOptions>;
export type CheckOptions = OneShot<TypeScriptCheckOptions>;

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

/** Run a TypeScript file from a mount, by its path inside the VM. Types are stripped, not checked. */
export function executeFile(
	path: string,
	options?: ExecuteFileOptions,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.executeFile(path, operationOptions),
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
