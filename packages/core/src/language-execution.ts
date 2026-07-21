import type { JsonValue } from "./session-api.js";

export type ExecutionSignal =
	| "SIGHUP"
	| "SIGINT"
	| "SIGQUIT"
	| "SIGTERM"
	| "SIGKILL"
	| "SIGSTOP"
	| "SIGCONT"
	| "SIGUSR1"
	| "SIGUSR2";

export interface LanguageExecutionOptions {
	executionId?: string;
	createIfMissing?: boolean;
	cwd?: string;
	env?: Record<string, string>;
	args?: string[];
	stdin?: string | Uint8Array;
	timeoutMs?: number;
	detached?: boolean;
	pty?: { cols?: number; rows?: number };
	signal?: AbortSignal;
	onStdout?: (chunk: Uint8Array) => void;
	onStderr?: (chunk: Uint8Array) => void;
}

export interface InlineExecutionOptions extends LanguageExecutionOptions {
	inputs?: Record<string, JsonValue>;
}

export interface JavaScriptExecutionOptions extends InlineExecutionOptions {
	format?: "module" | "commonjs";
	filePath?: string;
}

export type JavaScriptEvaluationOptions = Omit<
	JavaScriptExecutionOptions,
	"detached"
>;

export interface TypeScriptExecutionOptions extends InlineExecutionOptions {
	filePath?: string;
	tsconfigPath?: string;
	compilerOptions?: Record<string, JsonValue>;
}

export type TypeScriptEvaluationOptions = Omit<
	TypeScriptExecutionOptions,
	"detached"
>;

export interface TypeScriptFileExecutionOptions
	extends LanguageExecutionOptions {
	tsconfigPath?: string;
	compilerOptions?: Record<string, JsonValue>;
}

export interface TypeScriptCheckOptions {
	executionId?: string;
	createIfMissing?: boolean;
	cwd?: string;
	filePath?: string;
	tsconfigPath?: string;
	compilerOptions?: Record<string, JsonValue>;
	timeoutMs?: number;
	signal?: AbortSignal;
}

export interface NpmProjectInstallOptions
	extends Omit<LanguageExecutionOptions, "args" | "stdin" | "detached"> {
	frozen?: boolean;
}

export interface NpmPackageInstallOptions
	extends Omit<LanguageExecutionOptions, "args" | "stdin" | "detached"> {
	dev?: boolean;
	global?: boolean;
}

export interface PythonInstallOptions
	extends Omit<LanguageExecutionOptions, "args" | "stdin" | "detached"> {
	upgrade?: boolean;
	requirementsFile?: string;
	indexUrl?: string;
	extraIndexUrls?: string[];
}

export type ExecutionState =
	| "creating"
	| "idle"
	| "running"
	| "resetting"
	| "deleting"
	| "failed";
export type ExecutionOutcome =
	| "succeeded"
	| "failed"
	| "cancelled"
	| "timed_out";

export interface ExecutionDescriptor {
	executionId: string;
	generation: number;
	state: ExecutionState;
	retainedLanguage?: "javascript" | "python";
	processId?: string;
	pid?: number;
	createdAtMs: number;
	lastStartedAtMs?: number;
	lastCompletedAtMs?: number;
	lastOutcome?: ExecutionOutcome;
	lastExitCode?: number;
}

export interface DetachedExecution extends ExecutionDescriptor {
	detached: true;
}

export interface ExecutionErrorData {
	code: string;
	name: string;
	message: string;
	stack?: string;
	details?: JsonValue;
}

export type CodeOutput =
	| { type: "text" | "markdown" | "html" | "svg"; data: string }
	| { type: "json"; data: JsonValue }
	| { type: "png" | "jpeg"; data: string; encoding: "base64" };

interface CodeExecutionResultBase {
	executionId: string;
	generation: number;
	detached: false;
	exitCode?: number;
	stdout: string;
	stderr: string;
	stdoutTruncated: boolean;
	stderrTruncated: boolean;
	outputs: CodeOutput[];
}

export type CodeExecutionResult =
	| (CodeExecutionResultBase & {
			outcome: "succeeded";
			error?: never;
	  })
	| (CodeExecutionResultBase & {
			outcome: Exclude<ExecutionOutcome, "succeeded">;
			error: ExecutionErrorData;
	  });

export type CodeEvaluationResult<T = JsonValue> =
	| (CodeExecutionResultBase & {
			outcome: "succeeded";
			error?: never;
			value: T;
	  })
	| (CodeExecutionResultBase & {
			outcome: Exclude<ExecutionOutcome, "succeeded">;
			error: ExecutionErrorData;
			value?: never;
	  });

export interface TypeScriptDiagnostic {
	code: number;
	category: "error" | "warning" | "suggestion" | "message";
	message: string;
	filePath?: string;
	line?: number;
	column?: number;
}

export type TypeScriptCheckResult =
	| (CodeExecutionResultBase & {
			outcome: "succeeded";
			error?: never;
			hasErrors: boolean;
			diagnostics: TypeScriptDiagnostic[];
	  })
	| (CodeExecutionResultBase & {
			outcome: Exclude<ExecutionOutcome, "succeeded">;
			error: ExecutionErrorData;
			hasErrors?: never;
			diagnostics: TypeScriptDiagnostic[];
	  });

export interface ExecutionOutputEvent<TChunk = Uint8Array> {
	executionId: string;
	generation: number;
	processId?: string;
	sequence: number;
	channel: "stdout" | "stderr" | "pty";
	chunk: TChunk;
	timestampMs: number;
}

export interface ExecutionOutputPage<TChunk = Uint8Array> {
	executionId: string;
	generation: number;
	events: ExecutionOutputEvent<TChunk>[];
	nextCursor: string;
	hasMore: boolean;
	truncated: boolean;
}

export interface ExecutionCompletedEvent {
	executionId: string;
	generation: number;
	outcome: ExecutionOutcome;
	exitCode?: number;
	error?: ExecutionErrorData;
}
