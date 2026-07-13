import { join } from "node:path";
import { anthropic } from "@ai-sdk/anthropic";
import { createTypeScriptTools } from "@rivet-dev/agentos-typescript";
import { AgentOs, nodeModulesMount } from "@rivet-dev/agentos-core";
import { generateText, stepCountIs, tool } from "ai";
import { z } from "zod";

const vm = await AgentOs.create({
	defaultSoftware: false,
	mounts: [nodeModulesMount(join(process.cwd(), "node_modules"))],
	limits: { jsRuntime: { v8HeapLimitMb: 256, cpuTimeLimitMs: 5_000 } },
});
const ts = createTypeScriptTools({
	agentOs: vm,
});

try {
	const { text } = await generateText({
		model: anthropic("claude-sonnet-4-6"),
		prompt:
			"Write TypeScript that calculates the first 20 fibonacci numbers. Assign the result to module.exports.",
		stopWhen: stepCountIs(5),
		tools: {
			execute_typescript: tool({
				description:
					"Type-check TypeScript in a VM, compile it, then run the emitted JavaScript in the same VM. Return diagnostics when validation fails.",
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
					await vm.mkdir("/root", { recursive: true });
					await vm.writeFile("/root/generated.js", compiled.outputText);
					const executed = await vm.execArgv("node", [
						"-e",
						"const exportsValue = require('/root/generated.js'); console.log(JSON.stringify(exportsValue));",
					]);
					if (executed.exitCode !== 0) {
						throw new Error(
							executed.stderr.trim() ||
								`VM JavaScript exited ${executed.exitCode}`,
						);
					}

					return {
						ok: true,
						stage: "run",
						exports: JSON.parse(executed.stdout),
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
} finally {
	await vm.dispose();
}
