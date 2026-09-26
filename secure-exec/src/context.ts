import { randomUUID } from "node:crypto";
import type {
	AgentOs,
	CodeEvaluationResult,
	CodeExecutionResult,
	JavaScriptEvaluationOptions,
	JavaScriptExecutionOptions,
	JsonValue,
} from "@rivet-dev/agentos-core";

/**
 * Retained JavaScript state in a VM: variables and imports survive between
 * calls. A context runs one call at a time. For TypeScript in the same state,
 * pass `contextId` to `vm.typescript`.
 */
export interface Context extends AsyncDisposable {
	readonly contextId: string;
	execute(
		source: string,
		options?: Omit<JavaScriptExecutionOptions, "contextId">,
	): Promise<CodeExecutionResult>;
	evaluate<T = JsonValue>(
		source: string,
		options?: Omit<JavaScriptEvaluationOptions, "contextId">,
	): Promise<CodeEvaluationResult<T>>;
	/** Clear the retained state and keep the context. */
	reset(): Promise<void>;
	/** Delete the context. The VM is left running. */
	dispose(): Promise<void>;
}

/** Create a context in `vm`. A VM can hold many, and they run in parallel. */
export async function createContext(vm: AgentOs): Promise<Context> {
	const contextId = randomUUID();
	await vm.createContext(contextId);
	const dispose = () => vm.contexts.delete(contextId);
	return {
		contextId,
		execute: (source, options) =>
			vm.javascript.execute(source, { ...options, contextId }),
		evaluate: (source, options) =>
			vm.javascript.evaluate(source, { ...options, contextId }),
		reset: () => vm.contexts.reset(contextId),
		dispose,
		[Symbol.asyncDispose]: dispose,
	};
}
