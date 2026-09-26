import { describe, expect, it, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";

describe("bounded Core output replay", () => {
	it("language output remains scoped to its VM when a sibling reuses an execution ID", () => {
		let receive: (event: unknown) => void = () => {};
		const output = vi.fn();
		const vm = Reflect.construct(AgentOs, [
			{},
			{},
			[],
			[],
			{},
			{},
			{
				onEvent: (handler: typeof receive) => {
					receive = handler;
					return () => {};
				},
			},
			{ connectionId: "connection", sessionId: "session" },
			{ vmId: "own" },
		]) as AgentOs;
		const internal = vm as unknown as {
			_languageProcesses: Map<number, unknown>;
			_languageProcessIds: Map<string, number>;
		};
		internal._languageProcessIds.set("exec-1", 7);
		internal._languageProcesses.set(7, { outputHandlers: new Set([output]) });
		const payload = {
			type: "execution_output",
			event: {
				executionId: "exec-1",
				generation: 1n,
				processId: "process-1",
				sequence: 0n,
				channel: "Stdout",
				chunk: Uint8Array.of(65).buffer,
				timestampMs: 0n,
			},
		};
		const own = {
			scope: "vm",
			connection_id: "connection",
			session_id: "session",
			vm_id: "own",
		};
		for (const field of ["connection_id", "session_id", "vm_id"]) {
			receive({ ownership: { ...own, [field]: "other" }, payload });
		}
		expect(output).not.toHaveBeenCalled();
		receive({ ownership: own, payload });
		expect(output).toHaveBeenCalledOnce();
		expect(output.mock.calls[0][0]).toMatchObject({
			pid: 7,
			sequence: 0,
			timestampMs: 0,
		});
	});

	it("language replay honors byte pages without skipping the next chunk", async () => {
		const sendVmRequest = vi.fn(async (_session, _vm, input) => ({
			type: "execution_output_page",
			response: {
				events: [0, 1]
					.filter(
						(sequence) =>
							!input.request.cursor ||
							sequence >= Number(input.request.cursor.split(":")[1]),
					)
					.map((sequence) => ({
						executionId: "exec-1",
						generation: 1n,
						processId: null,
						sequence: BigInt(sequence),
						channel: "Stdout",
						chunk: new Uint8Array([sequence]).buffer,
						timestampMs: 1n,
					})),
				nextCursor: "1:2",
				hasMore: false,
				truncated: false,
			},
		}));
		const vm = Reflect.construct(AgentOs, [
			{},
			{},
			[],
			[],
			{},
			{},
			{
				onEvent: () => () => {},
				sendVmRequest,
			},
			{},
			{},
		]) as AgentOs;
		(
			vm as unknown as { _languageProcesses: Map<number, unknown> }
		)._languageProcesses.set(7, { executionId: "exec-1" });
		const first = await vm.process.readOutput(7, { maxEvents: 2, maxBytes: 1 });
		expect(first.nextCursor).toBe(0);
		expect(first.hasMore).toBe(true);
		const second = await vm.process.readOutput(7, {
			after: first.nextCursor!,
			maxBytes: 1,
		});
		expect(second.events.map((event) => event.sequence)).toEqual([1]);
		expect(second.nextCursor).toBe(1);
		expect(sendVmRequest.mock.calls[0][2].request.limit).toBe(2);
	});
	it("terminal admission includes pending exits and late waits retain launch failures", async () => {
		let reject!: (error: Error) => void;
		const wait = new Promise<number>((_resolve, fail) => {
			reject = fail;
		});
		const vm = Reflect.construct(AgentOs, [
			{ openShell: () => ({ wait: () => wait, kill() {} }) },
			{},
			[],
			[],
			{},
			{},
			{ onEvent: () => () => {} },
			{},
			{},
		]) as AgentOs;
		const internals = vm as unknown as {
			_pendingShellExitPromises: Set<Promise<number>>;
		};
		for (let index = 0; index < 1024; index++)
			internals._pendingShellExitPromises.add(new Promise(() => {}));
		expect(() => vm.terminal.open()).toThrow("terminal limit 1024");
		internals._pendingShellExitPromises.clear();
		const shell = vm.terminal.open();
		const error = Object.assign(new Error("execution denied"), {
			code: "EACCES",
		});
		const logged = vi.spyOn(console, "error").mockImplementation(() => {});
		try {
			const waiting = expect(vm.terminal.wait(shell.shellId)).rejects.toBe(
				error,
			);
			reject(error);
			await waiting;
			await expect(vm.terminal.wait(shell.shellId)).rejects.toBe(error);
			expect(internals._pendingShellExitPromises.size).toBe(0);
			expect(logged).toHaveBeenCalled();
		} finally {
			logged.mockRestore();
		}
	});
	it("the public process adapter delegates replay and cursor semantics to the sidecar", async () => {
		const kernel = {
			spawn: () => ({
				pid: 7,
				processId: "process-7",
				exitCode: null,
				wait: () => new Promise<number>(() => {}),
			}),
		};
		const sendVmRequest = vi.fn(async () => ({
			type: "process_output_page",
			response: {
				processId: "process-7",
				events: [
					{
						sequence: 9n,
						channel: "Stdout",
						chunk: new Uint8Array([1]).buffer,
						timestampMs: 10n,
					},
				],
				nextCursor: 9n,
				hasMore: true,
				truncated: false,
				exitCode: null,
			},
		}));
		const vm = Reflect.construct(AgentOs, [
			kernel,
			{},
			[],
			[],
			{},
			{},
			{ onEvent: () => () => {}, sendVmRequest },
			{},
			{},
		]) as AgentOs;
		await vm.process.spawn("echo", [], { output: { retainEvents: true } });
		const page = await vm.process.readOutput(7, {
			after: 8,
			maxEvents: 512,
			maxBytes: 1024 * 1024,
		});
		expect(page.nextCursor).toBe(9);
		expect(page.hasMore).toBe(true);
		expect(page.events[0]).toMatchObject({ sequence: 9, channel: "stdout" });
		expect(sendVmRequest.mock.calls[0][2]).toMatchObject({
			type: "read_process_output",
			process_id: "process-7",
			after: 8,
			max_events: 512,
			max_bytes: 1024 * 1024,
		});
		await vm.process.readOutput(7);
		expect(sendVmRequest.mock.calls[1][2]).toMatchObject({
			max_events: 0,
			max_bytes: 0,
		});
		await expect(
			vm.process.readOutput(7, { maxEvents: 0 }),
		).rejects.toMatchObject({
			code: "ERR_AGENTOS_RESOURCE_LIMIT",
		});
		await expect(
			vm.process.readOutput(7, { maxBytes: 0 }),
		).rejects.toMatchObject({
			code: "ERR_AGENTOS_RESOURCE_LIMIT",
		});
		expect(sendVmRequest).toHaveBeenCalledTimes(2);
	});
});
