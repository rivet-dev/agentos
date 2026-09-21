import type {
	CodeEvaluationResult,
	CodeExecutionResult,
	HostFunctionSchemas,
	JavaScriptEvaluationOptions,
	JavaScriptExecutionOptions,
	JsonValue,
	LanguageExecutionOptions,
} from "@rivet-dev/agentos-core";
import { type OneShot, run } from "./runtime.js";

export type {
	CodeEvaluationResult,
	CodeExecutionResult,
	ExecutionErrorData,
	ExecutionOutputOptions,
	HostFunction,
	HostFunctionCollection,
	HostFunctionCollections,
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

export type ExecuteOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<JavaScriptExecutionOptions, HOST_FUNCTIONS>;
export type EvaluateOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<JavaScriptEvaluationOptions, HOST_FUNCTIONS>;
export type ExecuteFileOptions<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
> = OneShot<LanguageExecutionOptions, HOST_FUNCTIONS>;

/** Run JavaScript for its side effects and captured output. */
export function execute<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	source: string,
	options?: ExecuteOptions<HOST_FUNCTIONS>,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.javascript.execute(source, operationOptions),
	);
}

/** Evaluate one JavaScript expression and return its JSON value. */
export function evaluate<
	T = JsonValue,
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	source: string,
	options?: EvaluateOptions<HOST_FUNCTIONS>,
): Promise<CodeEvaluationResult<T>> {
	return run(options, (vm, operationOptions) =>
		vm.javascript.evaluate<T>(source, operationOptions),
	);
}

/** Run a JavaScript file from a mount, by its path inside the VM. */
export function executeFile<
	HOST_FUNCTIONS extends HostFunctionSchemas = HostFunctionSchemas,
>(
	path: string,
	options?: ExecuteFileOptions<HOST_FUNCTIONS>,
): Promise<CodeExecutionResult> {
	return run(options, (vm, operationOptions) =>
		vm.javascript.executeFile(path, operationOptions),
	);
}
