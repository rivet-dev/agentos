import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createTypeScriptTools } from "@rivet-dev/agentos-typescript";
import { AgentOs, nodeModulesMount } from "@rivet-dev/agentos-core";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const workspaceRoot = resolve(
	fileURLToPath(new URL("../../..", import.meta.url)),
);
const testSidecar = join(workspaceRoot, "target/debug/agentos-sidecar");
if (!process.env.AGENTOS_SIDECAR_BIN && existsSync(testSidecar)) {
	process.env.AGENTOS_SIDECAR_BIN = testSidecar;
}

describe("@rivet-dev/agentos-typescript", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [nodeModulesMount(join(workspaceRoot, "node_modules"))],
			limits: { jsRuntime: { v8HeapLimitMb: 256, cpuTimeLimitMs: 5_000 } },
		});
	});

	afterEach(async () => {
		await vm?.dispose();
	});

	function createTools(compilerSpecifier?: string) {
		return createTypeScriptTools({
			agentOs: vm,
			...(compilerSpecifier === undefined ? {} : { compilerSpecifier }),
		});
	}

	it("typechecks a project with node types from node_modules", async () => {
		const tools = createTools();
		await vm.mkdir("/root/src", { recursive: true });
		await vm.writeFile(
			"/root/tsconfig.json",
			JSON.stringify({
				compilerOptions: {
					module: "nodenext",
					moduleResolution: "nodenext",
					target: "es2022",
					types: ["node"],
					skipLibCheck: true,
				},
				include: ["src/**/*.ts"],
			}),
		);
		await vm.writeFile(
			"/root/src/index.ts",
			'import { Buffer } from "node:buffer";\nexport const output: Buffer = Buffer.from("ok");\n',
		);

		const result = await tools.typecheckProject({ cwd: "/root" });

		expect(result).toEqual({
			success: true,
			diagnostics: [],
		});
	});

	it("compiles a project into the virtual filesystem and the output executes", async () => {
		const tools = createTools();
		await vm.mkdir("/root/src", { recursive: true });
		await vm.writeFile(
			"/root/tsconfig.json",
			JSON.stringify({
				compilerOptions: {
					module: "commonjs",
					target: "es2022",
					outDir: "/root/dist",
				},
				include: ["src/**/*.ts"],
			}),
		);
		await vm.writeFile(
			"/root/src/index.ts",
			"export const value: number = 7;\n",
		);

		const compileResult = await tools.compileProject({ cwd: "/root" });

		expect(compileResult).toEqual({
			success: true,
			diagnostics: [],
			emitSkipped: false,
			emittedFiles: ["/root/dist/index.js"],
		});
		expect(compileResult.emittedFiles).toContain("/root/dist/index.js");
		const emitted = new TextDecoder().decode(
			await vm.readFile("/root/dist/index.js"),
		);
		expect(emitted).toContain("exports.value = 7");

		const executed = await vm.execArgv("node", [
			"-e",
			"const value = require('/root/dist/index.js').value; console.log(JSON.stringify({ value }));",
		]);

		expect(executed.exitCode).toBe(0);
		expect(executed.stderr).toBe("");
		expect(JSON.parse(executed.stdout)).toEqual({ value: 7 });
	});

	it("typechecks a source string without mutating the filesystem", async () => {
		const tools = createTools();

		const result = await tools.typecheckSource({
			sourceText: "const value: string = 1;\n",
			filePath: "/root/input.ts",
		});

		expect(result.success).toBe(false);
		expect(
			result.diagnostics.some((diagnostic) => diagnostic.code === 2322),
		).toBe(true);
	});

	it("uses the caller-owned VM and removes its temporary runner files", async () => {
		const tools = createTools();
		await vm.writeFile("/tmp/caller-owned.txt", "still here");

		await expect(tools.typecheckSource({
			sourceText: "const value: number = 1;\n",
			filePath: "/root/input.ts",
		})).resolves.toEqual({
			success: true,
			diagnostics: [],
		});
		expect(new TextDecoder().decode(await vm.readFile("/tmp/caller-owned.txt"))).toBe("still here");
		expect(
			(await vm.readdir("/tmp")).filter((name) =>
				name.startsWith("agentos-typescript-"),
			),
		).toEqual([]);
	});

	it("compiles a source string to JavaScript text", async () => {
		const tools = createTools();

		const result = await tools.compileSource({
			sourceText: "export const value: number = 3;\n",
			filePath: "/root/input.ts",
			compilerOptions: {
				module: "commonjs",
				target: "es2022",
			},
		});

		expect(result.success).toBe(true);
		expect(result.diagnostics).toEqual([]);
		expect(result.outputText).toContain("exports.value = 3");
	});

	it("returns a diagnostic when the compiler module cannot be loaded", async () => {
		const brokenTools = createTools("typescript-does-not-exist");

		const result = await brokenTools.typecheckSource({
			sourceText: "export const value = 1;\n",
			filePath: "/root/input.ts",
		});

		expect(result.success).toBe(false);
		expect(result.diagnostics).toEqual([
			expect.objectContaining({
				category: "error",
				code: 0,
				message: expect.stringContaining("typescript-does-not-exist"),
			}),
		]);
	});
});
