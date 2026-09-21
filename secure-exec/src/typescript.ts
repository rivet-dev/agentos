import type {
	CodeEvaluationResult,
	CodeExecutionResult,
	HostFunctionSchemas,
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

export type ExecuteOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<TypeScriptExecutionOptions, HOST_FUNCTIONS>;
export type EvaluateOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<TypeScriptEvaluationOptions, HOST_FUNCTIONS>;
export type ExecuteFileOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<TypeScriptFileExecutionOptions, HOST_FUNCTIONS>;
export type CheckOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<TypeScriptCheckOptions, HOST_FUNCTIONS>;

/** Run TypeScript for its side effects and captured output. Types are stripped, not checked. */
export function execute<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	source: string,
	options?: ExecuteOptions<HOST_FUNCTIONS>,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.execute(source, operationOptions),
	);
}

/** Evaluate one TypeScript expression and return its JSON value. Types are stripped, not checked. */
export function evaluate<
	T = JsonValue,
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	source: string,
	options?: EvaluateOptions<HOST_FUNCTIONS>,
): Promise<CodeEvaluationResult<T>> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.evaluate<T>(source, operationOptions),
	);
}

/** Run a TypeScript file from a mount, by its path inside the VM. Types are stripped, not checked. */
export function executeFile<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	path: string,
	options?: ExecuteFileOptions<HOST_FUNCTIONS>,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.executeFile(path, operationOptions),
	);
}

/** Type-check TypeScript without running it. */
export function check<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	source: string,
	options?: CheckOptions<HOST_FUNCTIONS>,
): Promise<TypeScriptCheckResult> {
	return run(options, (vm, operationOptions) =>
		vm.typescript.check(source, operationOptions),
	);
}
