import { expect, test, vi } from "vitest";
import {
	type CodeExecutionProcessOutput,
	type CodeExecutionVm,
	spawnCodeProcess,
} from "../src/code-execution.js";

test("code capture accepts its exact byte budget and decodes split UTF-8", async () => {
	let output!: (event: CodeExecutionProcessOutput) => void;
	const vm = {
		spawn: () => ({ pid: 1 }),
		onProcessOutput: (_pid, handler) => {
			output = handler;
			return () => {};
		},
		waitProcess: async () => 0,
	} as CodeExecutionVm;
	const child = spawnCodeProcess(vm, "node", [], { maxCapturedOutputBytes: 2 });
	output({ pid: 1, stream: "stdout", data: Uint8Array.of(0xc3) });
	output({ pid: 1, stream: "stdout", data: Uint8Array.of(0xa9) });
	await expect(child.wait()).resolves.toMatchObject({
		stdout: "é",
		stderr: "",
	});
});

test("the combined code capture budget preserves streaming callbacks and reports a typed error", async () => {
	let output!: (event: CodeExecutionProcessOutput) => void;
	const unsubscribe = vi.fn();
	const onComplete = vi.fn();
	const vm = {
		spawn: () => ({ pid: 1 }),
		onProcessOutput: (_pid, handler) => {
			output = handler;
			return unsubscribe;
		},
		waitProcess: async () => 0,
	} as CodeExecutionVm;
	const onStdout = vi.fn();
	const onStderr = vi.fn();
	const child = spawnCodeProcess(vm, "node", [], {
		maxCapturedOutputBytes: 3,
		onStdout,
		onStderr,
		onComplete,
	});
	const encoder = new TextEncoder();
	output({ pid: 1, stream: "stdout", data: encoder.encode("ab") });
	output({ pid: 1, stream: "stderr", data: encoder.encode("cd") });
	await expect(child.wait()).rejects.toMatchObject({
		code: "ERR_AGENTOS_CAPTURED_OUTPUT_LIMIT",
		limitBytes: 3,
	});
	expect(onStdout).toHaveBeenCalledWith("ab");
	expect(onStderr).toHaveBeenCalledWith("cd");
	expect(unsubscribe).toHaveBeenCalledOnce();
	expect(onComplete).toHaveBeenCalledOnce();
});
