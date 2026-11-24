import { expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { NodeRuntime } from "../src/node-runtime.js";

test("NodeRuntime forwards WASM backend selection on every execution path", async () => {
	const wait = vi.fn().mockResolvedValue(0);
	const kernel = {
		exec: vi.fn().mockResolvedValue({ stdout: "", stderr: "", exitCode: 0 }),
		spawn: vi.fn().mockReturnValue({ wait }),
		writeFile: vi.fn().mockResolvedValue(undefined),
	};
	const runtime = Reflect.construct(NodeRuntime, [kernel]) as NodeRuntime;
	await runtime.exec("", { wasmBackend: "wasmtime" });
	await runtime.exec("", {
		wasmBackend: "wasmtime-threads",
		signal: new AbortController().signal,
	});
	await runtime.spawn("", { wasmBackend: "v8" });
	expect(kernel.exec.mock.calls[0][1]).toMatchObject({
		wasmBackend: "wasmtime",
	});
	expect(kernel.spawn.mock.calls.map((args) => args[2].wasmBackend)).toEqual([
		"wasmtime-threads",
		"v8",
	]);
});

test("unsupported native per-run CPU overrides reject before dispatching guest work", async () => {
	const kernel = { exec: vi.fn(), spawn: vi.fn(), writeFile: vi.fn() };
	const runtime = Reflect.construct(NodeRuntime, [kernel]) as NodeRuntime;
	const options = { cpuTimeLimitMs: 10 };
	for (const result of [
		runtime.exec("", options),
		runtime.spawn("", options),
		runtime.run("", options),
		runtime.execCommand("node", [], options),
	]) {
		await expect(result).rejects.toMatchObject({
			code: "ERR_AGENTOS_UNSUPPORTED_EXECUTION_OPTION",
		});
	}
	expect(() => runtime.spawnCommand("node", [], options)).toThrow(
		"cpuTimeLimitMs",
	);
	expect(kernel.writeFile).not.toHaveBeenCalled();
	expect(kernel.exec).not.toHaveBeenCalled();
	expect(kernel.spawn).not.toHaveBeenCalled();
});

test("deprecated AgentOs execution rejects unsupported CPU and timing overrides before dispatch", async () => {
	const result = { stdout: "", stderr: "", exitCode: 0 };
	const kernel = {
		exec: vi.fn().mockResolvedValue(result),
		execArgv: vi.fn().mockResolvedValue(result),
	};
	const sidecarClient = {
		onEvent: vi.fn(),
		onClose: vi.fn(),
	};
	const vm = Reflect.construct(AgentOs, [
		kernel,
		{},
		[],
		[],
		{},
		{},
		sidecarClient,
		{},
		{},
	]) as AgentOs;

	for (const options of [
		{ cpuTimeLimitMs: 10 },
		{ timingMitigation: "freeze" as const },
	]) {
		await expect(vm.exec("node", options)).rejects.toMatchObject({
			code: "ERR_AGENTOS_UNSUPPORTED_EXECUTION_OPTION",
		});
		await expect(vm.execArgv("node", [], options)).rejects.toMatchObject({
			code: "ERR_AGENTOS_UNSUPPORTED_EXECUTION_OPTION",
		});
	}
	expect(kernel.exec).not.toHaveBeenCalled();
	expect(kernel.execArgv).not.toHaveBeenCalled();
	await expect(vm.exec("node")).resolves.toEqual(result);
	await expect(
		vm.execArgv("node", [], {
			cpuTimeLimitMs: undefined,
			timingMitigation: "off",
		}),
	).resolves.toEqual(result);
});

test("resident output preserves split UTF-8 and rejects output beyond the request budget", async () => {
	const encode = (text: string) => new TextEncoder().encode(text);
	const exit = Promise.withResolvers<number>();
	const kill = vi.fn(() => exit.resolve(0));
	let firstRequest = true;
	const kernel = {
		writeFile: async () => {},
		spawn: vi.fn((_command, _args, options) => {
			const stdout: (chunk: Uint8Array) => void = options.onStdout;
			queueMicrotask(() => stdout(encode("__AGENTOS_RESIDENT_READY__\n")));
			return {
				kill,
				wait: () => exit.promise,
				writeStdin(line: string) {
					const { id } = JSON.parse(line);
					if (firstRequest) {
						firstRequest = false;
						stdout(Uint8Array.of(0xc3));
						stdout(Uint8Array.of(0xa9, 10));
						stdout(
							encode(
								`__AGENTOS_RESIDENT_RESULT__${JSON.stringify({ id, exitCode: 0 })}\n`,
							),
						);
					} else {
						stdout(encode("abc"));
					}
				},
			};
		}),
	};
	const runtime = Reflect.construct(NodeRuntime, [kernel]) as NodeRuntime;
	const runner = await runtime.createResidentRunner();
	try {
		await expect(runner.exec("small output")).resolves.toMatchObject({
			stdout: "é\n",
			exitCode: 0,
		});
		await expect(
			runner.exec("small budget", { maxCapturedOutputBytes: 2 }),
		).rejects.toMatchObject({
			code: "ERR_AGENTOS_CAPTURED_OUTPUT_LIMIT",
			limitBytes: 2,
		});
		expect(kill).toHaveBeenCalledWith(9);
	} finally {
		await runner.dispose();
	}
});
