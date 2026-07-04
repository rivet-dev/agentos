import { join } from "node:path";
import { anthropic } from "@ai-sdk/anthropic";
import { createTypeScriptTools } from "@secure-exec/typescript";
import { generateText, stepCountIs, tool } from "ai";
import {
	allowAll,
	createInMemoryFileSystem,
	createKernel,
	createNodeDriver,
	createNodeRuntime,
	createNodeRuntimeDriverFactory,
	nodeModulesMount,
} from "secure-exec";
import { z } from "zod";

const filesystem = createInMemoryFileSystem();
const systemDriver = createNodeDriver({
	filesystem,
	mounts: [nodeModulesMount(join(process.cwd(), "node_modules"))],
	permissions: allowAll,
});
const runtimeDriverFactory = createNodeRuntimeDriverFactory();
const ts = createTypeScriptTools({
	systemDriver,
	runtimeDriverFactory,
	memoryLimit: 256,
	cpuTimeLimitMs: 5000,
});

const { text } = await generateText({
	model: anthropic("claude-sonnet-4-6"),
	prompt:
		"Write TypeScript that calculates the first 20 fibonacci numbers. Assign the result to module.exports.",
	stopWhen: stepCountIs(5),
	tools: {
		execute_typescript: tool({
			description:
				"Type-check TypeScript in a sandbox, compile it, then run the emitted JavaScript in a sandbox. Return diagnostics when validation fails.",
			inputSchema: z.object({ code: z.string() }),
			execute: async ({ code }) => {
				const typecheck = await ts.typecheckSource({
					sourceText: code,
					filePath: "/root/generated.ts",
					compilerOptions: {
						module: "commonjs",
						target: "es2022",
					},
				});

				if (!typecheck.success) {
					return {
						ok: false,
						stage: "typecheck",
						diagnostics: typecheck.diagnostics,
					};
				}

				const compiled = await ts.compileSource({
					sourceText: code,
					filePath: "/root/generated.ts",
					compilerOptions: {
						module: "commonjs",
						target: "es2022",
					},
				});

				if (!compiled.success || !compiled.outputText) {
					return {
						ok: false,
						stage: "compile",
						diagnostics: compiled.diagnostics,
					};
				}

				try {
					await filesystem.mkdir("/root", { recursive: true });
					await filesystem.writeFile("/root/generated.js", compiled.outputText);
					const kernel = createKernel({
						filesystem,
						permissions: allowAll,
						syncFilesystemOnDispose: false,
					});
					let stdout = "";
					let stderr = "";
					try {
						await kernel.mount(createNodeRuntime());
						const child = kernel.spawn(
							"node",
							[
								"-e",
								"const exportsValue = require('/root/generated.js'); console.log(JSON.stringify(exportsValue));",
							],
							{
								onStdout: (chunk) => {
									stdout += Buffer.from(chunk).toString("utf8");
								},
								onStderr: (chunk) => {
									stderr += Buffer.from(chunk).toString("utf8");
								},
							},
						);
						const exitCode = await child.wait();
						if (exitCode !== 0) {
							throw new Error(
								stderr.trim() || `sandboxed JavaScript exited ${exitCode}`,
							);
						}
					} finally {
						await kernel.dispose();
					}

					return {
						ok: true,
						stage: "run",
						exports: JSON.parse(stdout),
					};
				} catch (error) {
					return {
						ok: false,
						stage: "run",
						errorMessage:
							error instanceof Error ? error.message : String(error),
					};
				}
			},
		}),
	},
});

console.log(text);
