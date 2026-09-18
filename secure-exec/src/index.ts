import type {
	CodeEvaluationResult,
	CodeExecutionResult,
	JavaScriptEvaluationOptions,
	JavaScriptExecutionOptions,
	JsonValue,
	LanguageExecutionOptions,
} from "@rivet-dev/agentos-core";
import { type OneShot, run } from "./runtime.js";

export type {
	Binding,
	Bindings,
	CodeEvaluationResult,
	CodeExecutionResult,
	ExecutionErrorData,
	ExecutionOutputOptions,
	HttpRequest,
	HttpResponse,
	JsonValue,
	LimitWarning,
	MountConfig,
	OutputCapture,
	Permissions,
	ProcessDescriptor,
	ProcessExit,
	SidecarRejectionDetail,
} from "@rivet-dev/agentos-core";
export {
	binding,
	bindings,
	createHostDirBackend,
	hostDirMount,
	KernelError,
	nodeModulesMount,
	SidecarProcessError,
	SidecarProcessExited,
	SidecarRejectedError,
	SidecarSilenceTimeout,
} from "@rivet-dev/agentos-core";
export type { Context } from "./context.js";
export {
	createVm,
	init,
	shutdown,
	type Vm,
	type VmOptions,
} from "./runtime.js";

// The functions below are one-shot conveniences: each call runs in a fresh VM
// that is disposed when the call finishes. For anything that should persist,
// create a VM with `createVm()` and call the same methods on it.

export type ExecuteOptions = OneShot<JavaScriptExecutionOptions>;
export type EvaluateOptions = OneShot<JavaScriptEvaluationOptions>;
export type ExecuteFileOptions = OneShot<LanguageExecutionOptions>;

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

/** Run a JavaScript file from a mount, by its path inside the VM. */
export function executeFile(
	path: string,
	options?: ExecuteFileOptions,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.javascript.executeFile(path, operationOptions),
	);
}
