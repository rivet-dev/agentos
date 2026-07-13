import type { AgentOs } from "@rivet-dev/agentos-core";

export interface TypeScriptDiagnostic {
	code: number;
	category: "error" | "warning" | "suggestion" | "message";
	message: string;
	filePath?: string;
	line?: number;
	column?: number;
}

export interface TypeCheckResult {
	success: boolean;
	diagnostics: TypeScriptDiagnostic[];
}

export interface ProjectCompileResult extends TypeCheckResult {
	emitSkipped: boolean;
	emittedFiles: string[];
}

export interface SourceCompileResult extends TypeCheckResult {
	outputText?: string;
	sourceMapText?: string;
}

export interface ProjectCompilerOptions {
	cwd?: string;
	configFilePath?: string;
}

export interface SourceCompilerOptions {
	sourceText: string;
	filePath?: string;
	cwd?: string;
	configFilePath?: string;
	compilerOptions?: Record<string, unknown>;
}

export interface TypeScriptToolsOptions {
	agentOs: AgentOs;
	compilerSpecifier?: string;
}

export interface TypeScriptTools {
	typecheckProject(options?: ProjectCompilerOptions): Promise<TypeCheckResult>;
	compileProject(
		options?: ProjectCompilerOptions,
	): Promise<ProjectCompileResult>;
	typecheckSource(options: SourceCompilerOptions): Promise<TypeCheckResult>;
	compileSource(options: SourceCompilerOptions): Promise<SourceCompileResult>;
}

type CompilerRequest =
	| {
			kind: "typecheckProject";
			compilerSpecifier: string;
			options: ProjectCompilerOptions;
	  }
	| {
			kind: "compileProject";
			compilerSpecifier: string;
			options: ProjectCompilerOptions;
	  }
	| {
			kind: "typecheckSource";
			compilerSpecifier: string;
			options: SourceCompilerOptions;
	  }
	| {
			kind: "compileSource";
			compilerSpecifier: string;
			options: SourceCompilerOptions;
	  };

type CompilerResponse =
	| TypeCheckResult
	| ProjectCompileResult
	| SourceCompileResult;
type RuntimeCompilerEnvelope =
	| { ok: true; result: CompilerResponse }
	| { ok: false; errorMessage?: string };

const DEFAULT_COMPILER_SPECIFIER = "typescript";
let nextRuntimeRequestId = 0;

export function createTypeScriptTools(
	options: TypeScriptToolsOptions,
): TypeScriptTools {
	return {
		typecheckProject: async (requestOptions = {}) =>
			runCompilerRequest<TypeCheckResult>(options, {
				kind: "typecheckProject",
				compilerSpecifier:
					options.compilerSpecifier ?? DEFAULT_COMPILER_SPECIFIER,
				options: requestOptions,
			}),
		compileProject: async (requestOptions = {}) =>
			runCompilerRequest<ProjectCompileResult>(options, {
				kind: "compileProject",
				compilerSpecifier:
					options.compilerSpecifier ?? DEFAULT_COMPILER_SPECIFIER,
				options: requestOptions,
			}),
		typecheckSource: async (requestOptions) =>
			runCompilerRequest<TypeCheckResult>(options, {
				kind: "typecheckSource",
				compilerSpecifier:
					options.compilerSpecifier ?? DEFAULT_COMPILER_SPECIFIER,
				options: requestOptions,
			}),
		compileSource: async (requestOptions) =>
			runCompilerRequest<SourceCompileResult>(options, {
				kind: "compileSource",
				compilerSpecifier:
					options.compilerSpecifier ?? DEFAULT_COMPILER_SPECIFIER,
				options: requestOptions,
			}),
	};
}

async function runCompilerRequest<TResult extends CompilerResponse>(
	options: TypeScriptToolsOptions,
	request: CompilerRequest,
): Promise<TResult> {
	try {
		return (await runCompilerInAgentOs(options.agentOs, request)) as TResult;
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return createFailureResult<TResult>(
			request.kind,
			`${request.kind} transport: ${message}`,
		);
	}
}

async function runCompilerInAgentOs(
	agentOs: AgentOs,
	request: CompilerRequest,
): Promise<CompilerResponse> {
	try {
		await agentOs.mkdir("/tmp", { recursive: true });
	} catch (error) {
		throw new Error(`failed to prepare TypeScript runner directory: ${String(error)}`, {
			cause: error,
		});
	}
	const requestId = `${Date.now()}-${nextRuntimeRequestId++}`;
	const requestPath = `/tmp/agentos-typescript-request-${requestId}.json`;
	const runnerPath = `/tmp/agentos-typescript-runner-${requestId}.cjs`;
	try {
		try {
			await agentOs.writeFile(requestPath, JSON.stringify(request));
			await agentOs.writeFile(
				runnerPath,
				buildCompilerRuntimeScript(requestPath),
			);
		} catch (error) {
			throw new Error(`failed to write TypeScript runner files: ${String(error)}`, {
				cause: error,
			});
		}
		let result;
		try {
			result = await agentOs.execArgv("node", [runnerPath], {
				...(request.options.cwd === undefined
					? {}
					: { cwd: request.options.cwd }),
			});
		} catch (error) {
			throw new Error(`TypeScript runner execution failed: ${String(error)}`, {
				cause: error,
			});
		}
		if (result.stdout.trim()) {
			let response;
			try {
				response = parseRuntimeResponse(result.stdout);
			} catch (error) {
				throw new Error(
					`failed to decode TypeScript runner response: ${String(error)}`,
					{ cause: error },
				);
			}
			return response;
		}
		if (result.exitCode !== 0) {
			throw new Error(
				`TypeScript runtime exited ${result.exitCode}${
					result.stderr.trim() ? `: ${result.stderr.trim()}` : ""
				}`,
			);
		}
		throw new Error("TypeScript runtime produced no response");
	} finally {
		try {
			await removeGuestFileIfExists(agentOs, requestPath);
			await removeGuestFileIfExists(agentOs, runnerPath);
		} catch (error) {
			throw new Error(
				`failed to remove TypeScript runner files: ${String(error)}`,
				{ cause: error },
			);
		}
	}
}

function createFailureResult<TResult extends CompilerResponse>(
	kind: CompilerRequest["kind"],
	errorMessage?: string,
): TResult {
	const diagnostic = {
		code: 0,
		category: "error" as const,
		message: normalizeCompilerFailureMessage(errorMessage),
	};

	if (kind === "compileProject") {
		return {
			success: false,
			diagnostics: [diagnostic],
			emitSkipped: true,
			emittedFiles: [],
		} as unknown as TResult;
	}

	if (kind === "compileSource") {
		return {
			success: false,
			diagnostics: [diagnostic],
		} as unknown as TResult;
	}

	return {
		success: false,
		diagnostics: [diagnostic],
	} as unknown as TResult;
}

function normalizeCompilerFailureMessage(errorMessage?: string): string {
	const message = (errorMessage ?? "TypeScript compiler failed").trim();
	if (/memory limit/i.test(message)) {
		return "TypeScript compiler exceeded sandbox memory limit";
	}
	if (/cpu time limit exceeded|timed out/i.test(message)) {
		return "TypeScript compiler exceeded sandbox CPU time limit";
	}
	return message;
}

function buildCompilerRuntimeScript(requestPath: string): string {
	return `
const fs = require("node:fs");
const path = require("node:path");

function loadTypeScriptCompiler(compilerSpecifier) {
	const specifier =
		compilerSpecifier === ${JSON.stringify(DEFAULT_COMPILER_SPECIFIER)}
			? compilerSpecifier
			: compilerSpecifier.startsWith("/")
				? compilerSpecifier
				: compilerSpecifier.startsWith("./") || compilerSpecifier.startsWith("../")
					? path.resolve(process.cwd(), compilerSpecifier)
					: compilerSpecifier;
	const imported = require(specifier);
	return imported.default ?? imported;
}

try {
	const request = JSON.parse(fs.readFileSync(${JSON.stringify(requestPath)}, "utf8"));
	const ts = loadTypeScriptCompiler(request.compilerSpecifier);
	const __name = (target) => target;
	const result = (${compilerRuntimeMain.toString()})(request, ts);
	process.stdout.write(JSON.stringify({ ok: true, result }));
} catch (error) {
	process.stdout.write(JSON.stringify({
		ok: false,
		errorMessage: error instanceof Error ? (error.stack ?? error.message) : String(error),
	}));
	process.exitCode = 1;
}
`;
}

function parseRuntimeResponse(stdout: string): CompilerResponse {
	return parseRuntimeEnvelope(
		JSON.parse(stdout.trim()) as RuntimeCompilerEnvelope,
	);
}

function parseRuntimeEnvelope(
	payload: RuntimeCompilerEnvelope,
): CompilerResponse {
	if (payload.ok) {
		return payload.result;
	}
	throw new Error(payload.errorMessage ?? "TypeScript runtime failed");
}

async function removeGuestFileIfExists(
	agentOs: AgentOs,
	targetPath: string,
): Promise<void> {
	if (await agentOs.exists(targetPath)) {
		await agentOs.delete(targetPath);
	}
}

function compilerRuntimeMain(
	request: CompilerRequest,
	ts: typeof import("typescript"),
): CompilerResponse {
	const fs = require("node:fs") as typeof import("node:fs");
	const path = require("node:path") as typeof import("node:path");

	function toDiagnostic(
		diagnostic: import("typescript").Diagnostic,
	): TypeScriptDiagnostic {
		const message = ts
			.flattenDiagnosticMessageText(diagnostic.messageText, "\n")
			.trim();
		const result: TypeScriptDiagnostic = {
			code: diagnostic.code,
			category: toDiagnosticCategory(diagnostic.category),
			message,
		};

		if (!diagnostic.file || diagnostic.start === undefined) {
			return result;
		}

		const { line, character } = diagnostic.file.getLineAndCharacterOfPosition(
			diagnostic.start,
		);
		result.filePath = diagnostic.file.fileName.replace(/\\/g, "/");
		result.line = line + 1;
		result.column = character + 1;
		return result;
	}

	function toDiagnosticCategory(
		category: import("typescript").DiagnosticCategory,
	): TypeScriptDiagnostic["category"] {
		switch (category) {
			case ts.DiagnosticCategory.Warning:
				return "warning";
			case ts.DiagnosticCategory.Suggestion:
				return "suggestion";
			case ts.DiagnosticCategory.Message:
				return "message";
			default:
				return "error";
		}
	}

	function hasErrors(diagnostics: TypeScriptDiagnostic[]): boolean {
		return diagnostics.some((diagnostic) => diagnostic.category === "error");
	}

	function convertCompilerOptions(
		compilerOptions: Record<string, unknown> | undefined,
		basePath: string,
	): import("typescript").CompilerOptions {
		if (!compilerOptions) {
			return {};
		}

		const converted = ts.convertCompilerOptionsFromJson(
			compilerOptions,
			basePath,
		);
		if (converted.errors.length > 0) {
			throw new Error(
				converted.errors
					.map((diagnostic) => toDiagnostic(diagnostic).message)
					.join("\n"),
			);
		}

		return converted.options;
	}

	function resolveProjectConfig(
		options: ProjectCompilerOptions,
		overrideCompilerOptions: import("typescript").CompilerOptions = {},
	) {
		const cwd = path.resolve(options.cwd ?? "/root");
		const configFilePath = options.configFilePath
			? path.resolve(cwd, options.configFilePath)
			: ts.findConfigFile(cwd, ts.sys.fileExists, "tsconfig.json");

		if (!configFilePath) {
			throw new Error(`Unable to find tsconfig.json from '${cwd}'`);
		}

		const configFile = ts.readConfigFile(configFilePath, ts.sys.readFile);
		if (configFile.error) {
			return {
				parsed: null,
				diagnostics: [toDiagnostic(configFile.error)],
			};
		}

		const parsed = ts.parseJsonConfigFileContent(
			configFile.config,
			ts.sys,
			path.dirname(configFilePath),
			overrideCompilerOptions,
			configFilePath,
		);

		return {
			parsed,
			diagnostics: parsed.errors.map(toDiagnostic),
		};
	}

	function createSourceProgram(
		options: SourceCompilerOptions,
		overrideCompilerOptions: import("typescript").CompilerOptions = {},
	) {
		const cwd = path.resolve(options.cwd ?? "/root");
		const filePath = path.resolve(
			cwd,
			options.filePath ?? "__secure_exec_typescript_input__.ts",
		);
		const projectCompilerOptions = options.configFilePath
			? resolveProjectConfig(
					{ cwd, configFilePath: options.configFilePath },
					overrideCompilerOptions,
				)
			: { parsed: null, diagnostics: [] as TypeScriptDiagnostic[] };

		if (projectCompilerOptions.diagnostics.length > 0) {
			return {
				filePath,
				program: null,
				host: null,
				diagnostics: projectCompilerOptions.diagnostics,
			};
		}

		const compilerOptions = {
			target: ts.ScriptTarget.ES2022,
			module: ts.ModuleKind.CommonJS,
			...projectCompilerOptions.parsed?.options,
			...convertCompilerOptions(options.compilerOptions, cwd),
			...overrideCompilerOptions,
		};
		const host = ts.createCompilerHost(compilerOptions);
		const normalizedFilePath = ts.sys.useCaseSensitiveFileNames
			? filePath
			: filePath.toLowerCase();
		const defaultGetSourceFile = host.getSourceFile.bind(host);
		const defaultReadFile = host.readFile.bind(host);
		const defaultFileExists = host.fileExists.bind(host);

		host.fileExists = (candidatePath) => {
			const normalizedCandidate = ts.sys.useCaseSensitiveFileNames
				? candidatePath
				: candidatePath.toLowerCase();
			return (
				normalizedCandidate === normalizedFilePath ||
				defaultFileExists(candidatePath)
			);
		};

		host.readFile = (candidatePath) => {
			const normalizedCandidate = ts.sys.useCaseSensitiveFileNames
				? candidatePath
				: candidatePath.toLowerCase();
			if (normalizedCandidate === normalizedFilePath) {
				return options.sourceText;
			}
			return defaultReadFile(candidatePath);
		};

		host.getSourceFile = (
			candidatePath,
			languageVersion,
			onError,
			shouldCreateNewSourceFile,
		) => {
			const normalizedCandidate = ts.sys.useCaseSensitiveFileNames
				? candidatePath
				: candidatePath.toLowerCase();
			if (normalizedCandidate === normalizedFilePath) {
				return ts.createSourceFile(
					candidatePath,
					options.sourceText,
					languageVersion,
					true,
				);
			}
			return defaultGetSourceFile(
				candidatePath,
				languageVersion,
				onError,
				shouldCreateNewSourceFile,
			);
		};

		return {
			filePath,
			host,
			program: ts.createProgram([filePath], compilerOptions, host),
			diagnostics: [] as TypeScriptDiagnostic[],
		};
	}

	switch (request.kind) {
		case "typecheckProject": {
			const { parsed, diagnostics } = resolveProjectConfig(request.options, {
				noEmit: true,
			});
			if (!parsed) {
				return {
					success: false,
					diagnostics,
				};
			}

			const program = ts.createProgram({
				rootNames: parsed.fileNames,
				options: parsed.options,
				projectReferences: parsed.projectReferences,
			});
			const combinedDiagnostics = ts
				.sortAndDeduplicateDiagnostics([
					...parsed.errors,
					...ts.getPreEmitDiagnostics(program),
				])
				.map(toDiagnostic);

			return {
				success: !hasErrors(combinedDiagnostics),
				diagnostics: combinedDiagnostics,
			};
		}

		case "compileProject": {
			const { parsed, diagnostics } = resolveProjectConfig(request.options);
			if (!parsed) {
				return {
					success: false,
					diagnostics,
					emitSkipped: true,
					emittedFiles: [],
				};
			}

			const program = ts.createProgram({
				rootNames: parsed.fileNames,
				options: parsed.options,
				projectReferences: parsed.projectReferences,
			});
			const emittedFiles: string[] = [];
			const emitResult = program.emit(undefined, (fileName, text) => {
				fs.mkdirSync(path.dirname(fileName), { recursive: true });
				fs.writeFileSync(fileName, text, "utf8");
				emittedFiles.push(fileName.replace(/\\/g, "/"));
			});
			const combinedDiagnostics = ts
				.sortAndDeduplicateDiagnostics([
					...parsed.errors,
					...ts.getPreEmitDiagnostics(program),
					...emitResult.diagnostics,
				])
				.map(toDiagnostic);

			return {
				success: !hasErrors(combinedDiagnostics),
				diagnostics: combinedDiagnostics,
				emitSkipped: emitResult.emitSkipped,
				emittedFiles,
			};
		}

		case "typecheckSource": {
			const { program, diagnostics } = createSourceProgram(request.options, {
				noEmit: true,
			});
			if (!program) {
				return {
					success: false,
					diagnostics,
				};
			}

			const combinedDiagnostics = ts
				.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program))
				.map(toDiagnostic);

			return {
				success: !hasErrors(combinedDiagnostics),
				diagnostics: combinedDiagnostics,
			};
		}

		case "compileSource": {
			const { program, diagnostics } = createSourceProgram(request.options);
			if (!program) {
				return {
					success: false,
					diagnostics,
				};
			}

			let outputText: string | undefined;
			let sourceMapText: string | undefined;
			const emitResult = program.emit(undefined, (fileName, text) => {
				if (
					fileName.endsWith(".js") ||
					fileName.endsWith(".mjs") ||
					fileName.endsWith(".cjs")
				) {
					outputText = text;
					return;
				}
				if (fileName.endsWith(".map")) {
					sourceMapText = text;
				}
			});
			const combinedDiagnostics = ts
				.sortAndDeduplicateDiagnostics([
					...ts.getPreEmitDiagnostics(program),
					...emitResult.diagnostics,
				])
				.map(toDiagnostic);

			return {
				success: !hasErrors(combinedDiagnostics),
				diagnostics: combinedDiagnostics,
				outputText,
				sourceMapText,
			};
		}
	}
}
