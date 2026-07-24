import {
	AgentOs,
	type AgentOsOptions,
	type CodeEvaluationResult,
	type CodeExecutionResult,
	createHostDirBackend,
	type DetachedExecution,
	type JavaScriptEvaluationOptions,
	type JavaScriptExecutionOptions,
	type JsonValue,
	type LanguageExecutionOptions,
	type NodeModulesMountConfig,
	type NpmPackageInstallOptions,
	type NpmProjectInstallOptions,
	nodeModulesMount,
	type TypeScriptCheckOptions,
	type TypeScriptCheckResult,
	type TypeScriptEvaluationOptions,
	type TypeScriptExecutionOptions,
	type TypeScriptFileExecutionOptions,
} from "@rivet-dev/agentos-core";

export type * from "@rivet-dev/agentos-core";
export {
	binding,
	bindings,
	createHostDirBackend,
	defineSoftware,
	nodeModulesMount,
} from "@rivet-dev/agentos-core";

export interface JavaScriptRuntimeOptions extends AgentOsOptions {
	nodeModules?: string | NodeModulesMountConfig;
	cwd?: string;
	env?: Record<string, string>;
	files?: Record<string, string | Uint8Array>;
}

export interface TypeScriptTools {
	execute(
		source: string,
		options?: TypeScriptExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	execute(
		source: string,
		options: TypeScriptExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	evaluate<T = JsonValue>(
		expression: string,
		options?: TypeScriptEvaluationOptions,
	): Promise<CodeEvaluationResult<T>>;
	executeFile(
		path: string,
		options?: TypeScriptFileExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	executeFile(
		path: string,
		options: TypeScriptFileExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	check(
		source: string,
		options?: TypeScriptCheckOptions,
	): Promise<TypeScriptCheckResult>;
	checkProject(
		options?: Omit<TypeScriptCheckOptions, "filePath" | "compilerOptions">,
	): Promise<TypeScriptCheckResult>;
}

export class JavaScriptRuntime {
	readonly typescript: TypeScriptTools;

	private constructor(
		readonly vm: AgentOs,
		private readonly defaultCwd: string,
		private readonly defaultEnv: Record<string, string>,
	) {
		this.typescript = {
			execute: ((source, options = {}) =>
				this.vm.javascript.typescript.execute(
					source,
					this.withDefaults(options) as never,
				)) as TypeScriptTools["execute"],
			evaluate: (expression, options = {}) =>
				this.vm.javascript.typescript.evaluate(
					expression,
					this.withDefaults(options),
				),
			executeFile: ((path, options = {}) =>
				this.vm.javascript.typescript.executeFile(
					path,
					this.withDefaults(options) as never,
				)) as TypeScriptTools["executeFile"],
			check: (source, options = {}) =>
				this.vm.javascript.typescript.check(
					source,
					this.withDefaults(options),
				),
			checkProject: (options = {}) =>
				this.vm.javascript.typescript.checkProject(
					this.withDefaults(options),
				),
		};
	}

	static async create(
		options: JavaScriptRuntimeOptions = {},
	): Promise<JavaScriptRuntime> {
		const {
			nodeModules,
			cwd = "/workspace",
			env = {},
			files = {},
			...agentOsOptions
		} = options;
		const mounts = [...(agentOsOptions.mounts ?? [])];
		if (nodeModules !== undefined) {
			mounts.push(
				typeof nodeModules === "string"
					? nodeModulesMount(nodeModules)
					: nodeModules,
			);
		}
		const vm = await AgentOs.create({ ...agentOsOptions, mounts });
		try {
			for (const [path, contents] of Object.entries(files)) {
				await vm.filesystem.writeFile(path, contents);
			}
			return new JavaScriptRuntime(vm, cwd, env);
		} catch (error) {
			await vm.dispose();
			throw error;
		}
	}

	execute(
		source: string,
		options?: JavaScriptExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	execute(
		source: string,
		options: JavaScriptExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	execute(
		source: string,
		options: JavaScriptExecutionOptions = {},
	): Promise<CodeExecutionResult | DetachedExecution> {
		return this.vm.javascript.execute(
			source,
			this.withDefaults(options) as never,
		);
	}

	evaluate<T = JsonValue>(
		expression: string,
		options: JavaScriptEvaluationOptions = {},
	): Promise<CodeEvaluationResult<T>> {
		return this.vm.javascript.evaluate<T>(
			expression,
			this.withDefaults(options),
		);
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
		return this.vm.javascript.executeFile(
			path,
			this.withDefaults(options) as never,
		);
	}

	installNpmPackages(
		options?: NpmProjectInstallOptions,
	): Promise<CodeExecutionResult>;
	installNpmPackages(
		packages: string | string[],
		options?: NpmPackageInstallOptions,
	): Promise<CodeExecutionResult>;
	installNpmPackages(
		packagesOrOptions: string | string[] | NpmProjectInstallOptions = {},
		options: NpmPackageInstallOptions = {},
	): Promise<CodeExecutionResult> {
		return typeof packagesOrOptions === "string" ||
			Array.isArray(packagesOrOptions)
			? this.vm.javascript.npm.install(
					packagesOrOptions,
					this.withDefaults(options),
				)
			: this.vm.javascript.npm.install(
					this.withDefaults(packagesOrOptions),
				);
	}

	executeNpmScript(
		script: string,
		options?: LanguageExecutionOptions & { detached?: false },
	): Promise<CodeExecutionResult>;
	executeNpmScript(
		script: string,
		options: LanguageExecutionOptions & { detached: true },
	): Promise<DetachedExecution>;
	executeNpmScript(
		script: string,
		options: LanguageExecutionOptions = {},
	): Promise<CodeExecutionResult | DetachedExecution> {
		return this.vm.javascript.npm.runScript(
			script,
			this.withDefaults(options) as never,
		);
	}

	executeNpmPackage(
		packageSpec: string,
		options?: LanguageExecutionOptions & {
			detached?: false;
			binary?: string;
		},
	): Promise<CodeExecutionResult>;
	executeNpmPackage(
		packageSpec: string,
		options: LanguageExecutionOptions & {
			detached: true;
			binary?: string;
		},
	): Promise<DetachedExecution>;
	executeNpmPackage(
		packageSpec: string,
		options: LanguageExecutionOptions & { binary?: string } = {},
	): Promise<CodeExecutionResult | DetachedExecution> {
		return this.vm.javascript.npm.runPackage(
			packageSpec,
			this.withDefaults(options) as never,
		);
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
