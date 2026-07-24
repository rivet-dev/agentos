import {
	AgentOs,
	type AgentOsOptions,
	type CodeEvaluationResult,
	type CodeExecutionResult,
	type DetachedExecution,
	type InlineExecutionOptions,
	type JsonValue,
	type LanguageExecutionOptions,
	type PythonInstallOptions,
} from "@rivet-dev/agentos-core";

export type * from "@rivet-dev/agentos-core";
export {
	binding,
	bindings,
	createHostDirBackend,
	defineSoftware,
} from "@rivet-dev/agentos-core";

export interface PythonRuntimeOptions extends AgentOsOptions {
	cwd?: string;
	env?: Record<string, string>;
	files?: Record<string, string | Uint8Array>;
}

export class PythonRuntime {
	private constructor(
		readonly vm: AgentOs,
		private readonly defaultCwd: string,
		private readonly defaultEnv: Record<string, string>,
	) {}

	static async create(
		options: PythonRuntimeOptions = {},
	): Promise<PythonRuntime> {
		const {
			cwd = "/workspace",
			env = {},
			files = {},
			...agentOsOptions
		} = options;
		const vm = await AgentOs.create(agentOsOptions);
		try {
			for (const [path, contents] of Object.entries(files)) {
				await vm.filesystem.writeFile(path, contents);
			}
			return new PythonRuntime(vm, cwd, env);
		} catch (error) {
			await vm.dispose();
			throw error;
		}
	}

	execute(
		source: string,
		options?: InlineExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	execute(
		source: string,
		options: InlineExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	execute(
		source: string,
		options: InlineExecutionOptions = {},
	): Promise<CodeExecutionResult | DetachedExecution> {
		return this.vm.python.execute(
			source,
			this.withDefaults(options) as never,
		);
	}

	evaluate<T = JsonValue>(
		expression: string,
		options: Omit<InlineExecutionOptions, "detached"> = {},
	): Promise<CodeEvaluationResult<T>> {
		return this.vm.python.evaluate<T>(expression, this.withDefaults(options));
	}

	executeFile(
		path: string,
		options?: LanguageExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	executeFile(
		path: string,
		options: LanguageExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	executeFile(
		path: string,
		options: LanguageExecutionOptions = {},
	): Promise<CodeExecutionResult | DetachedExecution> {
		return this.vm.python.executeFile(
			path,
			this.withDefaults(options) as never,
		);
	}

	executeModule(
		module: string,
		options?: LanguageExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	executeModule(
		module: string,
		options: LanguageExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	executeModule(
		module: string,
		options: LanguageExecutionOptions = {},
	): Promise<CodeExecutionResult | DetachedExecution> {
		return this.vm.python.executeModule(
			module,
			this.withDefaults(options) as never,
		);
	}

	install(options?: PythonInstallOptions): Promise<CodeExecutionResult>;
	install(
		packages: string | string[],
		options?: PythonInstallOptions,
	): Promise<CodeExecutionResult>;
	install(
		packagesOrOptions: string | string[] | PythonInstallOptions = {},
		options: PythonInstallOptions = {},
	): Promise<CodeExecutionResult> {
		return typeof packagesOrOptions === "string" ||
			Array.isArray(packagesOrOptions)
			? this.vm.python.install(
					packagesOrOptions,
					this.withDefaults(options),
				)
			: this.vm.python.install(this.withDefaults(packagesOrOptions));
	}

	dispose(): Promise<void> {
		return this.vm.dispose();
	}

	private withDefaults<
		T extends { cwd?: string; env?: Record<string, string> },
	>(options: T): T {
		return {
			...options,
			cwd: options.cwd ?? this.defaultCwd,
			env: { ...this.defaultEnv, ...options.env },
		};
	}
}
