import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";
import { describe, expect, test } from "vitest";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const readmePath = resolve(packageRoot, "README.md");
const checkedExamplePath = resolve(
	packageRoot,
	"../../examples/quickstart/agent-session/index.ts",
);

const expectedPrompt = "Write a hello world in TypeScript";
const piDescriptor = { packagePath: "/test/pi.aospkg" };

function removeFinalNewline(value: string): string {
	if (value.endsWith("\r\n")) return value.slice(0, -2);
	if (value.endsWith("\n")) return value.slice(0, -1);
	return value;
}

function extractQuickStartProgram(): string {
	const readme = readFileSync(readmePath, "utf8");
	const heading = readme.indexOf("## Quick Start");
	if (heading === -1)
		throw new Error("README is missing its Quick Start section");
	const nextHeading = readme.indexOf(
		"\n## ",
		heading + "## Quick Start".length,
	);
	const section = readme.slice(
		heading,
		nextHeading === -1 ? undefined : nextHeading,
	);
	const programs = [
		...section.matchAll(/```typescript[^\S\r\n]*\r?\n([\s\S]*?)^```\s*$/gm),
	];
	if (programs.length !== 1) {
		throw new Error(
			`README Quick Start must contain exactly one TypeScript program; found ${programs.length}`,
		);
	}
	return removeFinalNewline(programs[0][1]);
}

function removeInjectedImports(source: string): string {
	const allowedImports = new Set([
		'import pi from "@agentos-software/pi";',
		'import { AgentOs } from "@rivet-dev/agentos-core";',
	]);
	const imports = source.match(/^import .*;$/gm) ?? [];
	for (const statement of imports) {
		if (!allowedImports.has(statement)) {
			throw new Error(
				`README Quick Start has an unexpected import: ${statement}`,
			);
		}
	}
	if (!imports.includes('import { AgentOs } from "@rivet-dev/agentos-core";')) {
		throw new Error("README Quick Start must import AgentOs");
	}
	if (!imports.includes('import pi from "@agentos-software/pi";')) {
		throw new Error("README Quick Start must import the Pi package descriptor");
	}
	return source.replace(/^import .*;\s*$/gm, "");
}

function extractCheckedExample(): string {
	const example = readFileSync(checkedExamplePath, "utf8");
	const startMarker = "// docs:start core-readme-quickstart";
	const endMarker = "// docs:end core-readme-quickstart";
	const starts = example.split(startMarker).length - 1;
	const ends = example.split(endMarker).length - 1;
	if (starts !== 1 || ends !== 1) {
		throw new Error(
			`checked example must contain exactly one marker pair; found ${starts} starts and ${ends} ends`,
		);
	}
	const start = example.indexOf(startMarker) + startMarker.length;
	const end = example.indexOf(endMarker, start);
	const markedRegion = example.slice(start, end);
	const content = markedRegion.startsWith("\r\n")
		? markedRegion.slice(2)
		: markedRegion.startsWith("\n")
			? markedRegion.slice(1)
			: undefined;
	if (content === undefined) {
		throw new Error("checked example start marker must end its own line");
	}
	return removeFinalNewline(content);
}

function executeQuickStart(options: { promptError?: Error } = {}) {
	const calls: Array<{ method: string; args: unknown[] }> = [];
	const lifecycle: string[] = [];
	const projectedSoftware = new Set<unknown>();
	let releaseDispose: (() => void) | undefined;
	const disposeGate = new Promise<void>((resolveDispose) => {
		releaseDispose = resolveDispose;
	});
	const vm = {
		async createSession(agent: string, options?: unknown) {
			calls.push({ method: "createSession", args: [agent, options] });
			if (agent === "pi" && !projectedSoftware.has(piDescriptor)) {
				throw new Error("unknown agent type: pi");
			}
			return { sessionId: "session-1" };
		},
		async prompt(sessionId: string, prompt: string) {
			calls.push({ method: "prompt", args: [sessionId, prompt] });
			if (options.promptError) throw options.promptError;
			return { text: "hello from Pi" };
		},
		async closeSession(sessionId: string) {
			calls.push({ method: "closeSession", args: [sessionId] });
			lifecycle.push("close:start");
			await Promise.resolve();
			lifecycle.push("close:end");
		},
		async dispose() {
			calls.push({ method: "dispose", args: [] });
			lifecycle.push("dispose:start");
			await disposeGate;
			lifecycle.push("dispose:end");
		},
	};
	const AgentOs = {
		async create(options?: { software?: unknown[] }) {
			calls.push({ method: "create", args: [options] });
			for (const software of options?.software ?? [])
				projectedSoftware.add(software);
			return vm;
		},
	};
	const output: unknown[][] = [];
	const testConsole = { log: (...args: unknown[]) => output.push(args) };
	const testProcess = { env: { ANTHROPIC_API_KEY: "test-api-key" } };
	const source = removeInjectedImports(extractQuickStartProgram());
	const javascript = ts.transpileModule(source, {
		compilerOptions: {
			module: ts.ModuleKind.ESNext,
			target: ts.ScriptTarget.ES2022,
		},
	}).outputText;
	const AsyncFunction = Object.getPrototypeOf(async () => {})
		.constructor as new (
		...args: string[]
	) => (...args: unknown[]) => Promise<void>;
	const run = new AsyncFunction(
		"AgentOs",
		"pi",
		"process",
		"console",
		javascript,
	);
	return {
		calls,
		lifecycle,
		output,
		releaseDispose: () => releaseDispose?.(),
		completion: run(AgentOs, piDescriptor, testProcess, testConsole),
	};
}

async function waitForDisposeStart(lifecycle: string[]): Promise<void> {
	for (let attempt = 0; attempt < 10; attempt++) {
		if (lifecycle.includes("dispose:start")) return;
		await Promise.resolve();
	}
	throw new Error("README Quick Start did not reach VM disposal");
}

describe("Core README Quick Start", () => {
	test("stays byte-for-byte aligned with the checked example", () => {
		expect(extractQuickStartProgram()).toBe(extractCheckedExample());
	});

	test("projects Pi before creating the documented session", async () => {
		const execution = executeQuickStart();
		let completed = false;
		void execution.completion.then(
			() => {
				completed = true;
			},
			() => {
				completed = true;
			},
		);
		await waitForDisposeStart(execution.lifecycle);
		expect(completed).toBe(false);
		execution.releaseDispose();
		await execution.completion;

		expect(execution.calls).toEqual([
			{ method: "create", args: [{ software: [piDescriptor] }] },
			{
				method: "createSession",
				args: ["pi", { env: { ANTHROPIC_API_KEY: "test-api-key" } }],
			},
			{ method: "prompt", args: ["session-1", expectedPrompt] },
			{ method: "closeSession", args: ["session-1"] },
			{ method: "dispose", args: [] },
		]);
		expect(execution.output).toEqual([["hello from Pi"]]);
		expect(execution.lifecycle).toEqual([
			"close:start",
			"close:end",
			"dispose:start",
			"dispose:end",
		]);
	});

	test("closes the session and VM before propagating a prompt failure", async () => {
		const promptError = new Error("prompt failed");
		const execution = executeQuickStart({ promptError });
		let completed = false;
		void execution.completion.then(
			() => {
				completed = true;
			},
			() => {
				completed = true;
			},
		);

		await waitForDisposeStart(execution.lifecycle);
		expect(completed).toBe(false);
		execution.releaseDispose();
		await expect(execution.completion).rejects.toBe(promptError);
		expect(execution.calls).toEqual([
			{ method: "create", args: [{ software: [piDescriptor] }] },
			{
				method: "createSession",
				args: ["pi", { env: { ANTHROPIC_API_KEY: "test-api-key" } }],
			},
			{ method: "prompt", args: ["session-1", expectedPrompt] },
			{ method: "closeSession", args: ["session-1"] },
			{ method: "dispose", args: [] },
		]);
		expect(execution.lifecycle).toEqual([
			"close:start",
			"close:end",
			"dispose:start",
			"dispose:end",
		]);
		expect(execution.output).toEqual([]);
	});
});
