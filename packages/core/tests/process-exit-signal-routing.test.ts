import { describe, expect, it, vi } from "vitest";
import { AgentOs, type ProcessOutput } from "../src/agent-os.js";
import type { ProcessExit } from "../src/language-execution.js";
import type { ManagedProcess } from "../src/runtime-compat.js";
import { SidecarKernelProxy } from "../src/sidecar/rpc-client.js";

function vmWithKernel(kernel: unknown): AgentOs {
	return Reflect.construct(AgentOs, [
		kernel,
		{},
		[],
		[],
		{},
		{},
		{
			onEvent: () => () => {},
			onClose: () => () => {},
		},
		{ connectionId: "connection", sessionId: "session" },
		{ vmId: "vm" },
	]) as AgentOs;
}

describe("observed terminal reason routing", () => {
	it.each([
		[143, 15],
		[143, null],
		[0, null],
	] as const)("preserves actual (%i,%s) after a TERM request", async (code, signal) => {
		let settle!: (code: number) => void;
		const waiting = new Promise<number>((resolve) => {
			settle = resolve;
		});
		let exitCode: number | null = null;
		let exitSignal: number | null = null;
		const process: ManagedProcess = {
			pid: 7,
			kill: vi.fn(),
			writeStdin: async () => {},
			closeStdin: async () => {},
			wait: () => waiting,
			get exitCode() {
				return exitCode;
			},
			get exitSignal() {
				return exitSignal;
			},
		};
		const vm = vmWithKernel({});
		const internal = vm as unknown as {
			_trackProcess(
				process: ManagedProcess,
				command: string,
				args: string[],
				retain: boolean,
				output: Set<(event: ProcessOutput) => void>,
				exits: Set<(event: ProcessExit) => void>,
			): void;
		};
		const received: ProcessExit[] = [];
		internal._trackProcess(
			process,
			"node",
			[],
			true,
			new Set(),
			new Set([(exit) => received.push(exit)]),
		);
		const pending = vm.process.wait(7);
		await vm.process.signal(7, "SIGTERM");
		exitCode = code;
		exitSignal = signal;
		settle(code);
		const exit = await pending;
		expect(exit).toEqual({
			pid: 7,
			exitCode: code,
			outcome: signal ? "signalled" : "exited",
			...(signal ? { exitSignal: signal, signal: "SIGTERM" } : {}),
		});
		expect(received).toEqual([exit]);
		expect(await vm.process.wait(7)).toEqual(exit);
		expect((await vm.process.get(7)).exitSignal).toBe(signal ?? undefined);
	});

	it.each([
		[143, 15],
		[143, null],
		[0, null],
	] as const)("replay atomically completes a tracked adapter with (%i,%s)", (code, signal) => {
		const proxy = Object.create(
			SidecarKernelProxy.prototype,
		) as SidecarKernelProxy;
		const entry = {
			pid: 7,
			exitCode: null,
			exitSignal: null,
			confirmExit: vi.fn(),
			wakeWait: vi.fn(),
		};
		Object.assign(proxy, {
			trackedProcessesById: new Map([["process", entry]]),
			updateTrackedProcessSnapshot: vi.fn(),
			releaseProcessTrackingAfterDrain: async () => {},
		});
		proxy.reconcileReplayExit("other", 0, null);
		expect(entry.exitCode).toBeNull();
		proxy.reconcileReplayExit("process", code, signal);
		expect(entry).toMatchObject({ exitCode: code, exitSignal: signal });
		proxy.reconcileReplayExit("process", 9, 9);
		expect(entry).toMatchObject({ exitCode: code, exitSignal: signal });
	});

	it.each([
		[143, 15],
		[143, null],
		[0, null],
	] as const)("shell wait and callbacks retain full (%i,%s) after removal", async (exitCode, signal) => {
		let settle!: (code: number) => void;
		const waiting = new Promise<number>((resolve) => {
			settle = resolve;
		});
		const handle = {
			pid: 7,
			wait: () => waiting,
			exitSignal: signal,
			kill: vi.fn(),
		};
		const vm = vmWithKernel({ openShell: () => handle });
		const { shellId } = vm.terminal.open();
		const callback = vi.fn();
		vm.onShellExit(shellId, callback);
		const pending = vm.terminal.waitExit(shellId);
		settle(exitCode);
		const exit = await pending;
		expect(exit).toEqual({
			shellId,
			exitCode,
			...(signal ? { exitSignal: signal } : {}),
		});
		expect(await vm.terminal.wait(shellId)).toBe(exitCode);
		expect(await vm.terminal.waitExit(shellId)).toEqual(exit);
		expect(callback).toHaveBeenCalledTimes(1);
		expect(callback).toHaveBeenCalledWith(exit);
		const late = vi.fn();
		vm.onShellExit(shellId, late);
		expect(late).toHaveBeenCalledTimes(1);
		expect(late).toHaveBeenCalledWith(exit);
	});
	it.each([
		[143, 15],
		[143, null],
		[0, null],
	] as const)("language completion preserves actual (%i,%s) independently of cancellation outcome", async (code, signal) => {
		const vm = vmWithKernel({});
		let settle!: (result: unknown) => void;
		const waiting = new Promise((resolve) => {
			settle = resolve;
		});
		const received: ProcessExit[] = [];
		const internal = vm as unknown as {
			_languageProcesses: Map<number, unknown>;
			_languageProcessIds: Map<string, number>;
			_handleSidecarEvent(event: unknown): void;
		};
		Object.assign(vm, {
			_waitExecutionResult: () => waiting,
			_signalExecution: async () => {},
		});
		internal._languageProcesses.set(7, {
			executionId: "execution",
			descriptor: {
				pid: 7,
				state: "running",
				language: "javascript",
				startedAtMs: 0,
			},
			outputHandlers: new Set(),
			exitHandlers: new Set([(exit: ProcessExit) => received.push(exit)]),
		});
		internal._languageProcessIds.set("execution", 7);
		const pending = vm.process.wait(7);
		await vm.process.signal(7, "SIGTERM");
		internal._handleSidecarEvent({
			ownership: {
				scope: "vm",
				connection_id: "connection",
				session_id: "session",
				vm_id: "vm",
			},
			payload: {
				type: "execution_completed",
				event: {
					executionId: "execution",
					generation: 1n,
					outcome: "Cancelled",
					exitCode: code,
					exitSignal: signal,
					error: null,
				},
			},
		});
		settle({
			outcome: "cancelled",
			exitCode: code,
			...(signal ? { exitSignal: signal } : {}),
		});
		const exit = await pending;
		expect(exit).toEqual({
			pid: 7,
			exitCode: code,
			outcome: signal ? "signalled" : "exited",
			...(signal ? { exitSignal: signal, signal: "SIGTERM" } : {}),
		});
		expect(received).toEqual([exit]);
		expect((await vm.process.get(7)).exitSignal).toBe(signal ?? undefined);
	});

	it("language completion without a confirmed code rejects wait without emitting a fabricated exit", async () => {
		const vm = vmWithKernel({});
		const exits = vi.fn();
		Object.assign(vm, {
			_waitExecutionResult: async () => ({ outcome: "cancelled" }),
		});
		const internal = vm as unknown as {
			_languageProcesses: Map<number, unknown>;
		};
		internal._languageProcesses.set(7, {
			executionId: "execution",
			descriptor: { pid: 7, state: "running", startedAtMs: 0 },
			exitHandlers: new Set([exits]),
		});
		await expect(vm.process.wait(7)).rejects.toMatchObject({
			code: "termination_failed",
		});
		expect(exits).not.toHaveBeenCalled();
	});
	it("a pending process wait retains full observed status when VM disposal retires its registry first", async () => {
		let settle!: (code: number) => void;
		const waiting = new Promise<number>((resolve) => {
			settle = resolve;
		});
		let exitCode: number | null = null;
		let exitSignal: number | null = null;
		const process: ManagedProcess = {
			pid: 7,
			writeStdin: async () => {},
			closeStdin: async () => {},
			kill: vi.fn(),
			wait: () => waiting,
			get exitCode() {
				return exitCode;
			},
			get exitSignal() {
				return exitSignal;
			},
		};
		const vm = vmWithKernel({
			dispose: async () => {
				exitCode = 143;
				exitSignal = 15;
				settle(143);
			},
		});
		Object.assign(vm, { _cronManager: { dispose: () => {} } });
		const internal = vm as unknown as {
			_trackProcess: (...args: unknown[]) => void;
			_processes: Map<number, unknown>;
		};
		internal._trackProcess(process, "node", [], true, new Set(), new Set());
		const pending = vm.process.wait(7);
		await vm.dispose();
		expect(await pending).toEqual({
			pid: 7,
			exitCode: 143,
			exitSignal: 15,
			signal: "SIGTERM",
			outcome: "signalled",
		});
		expect(internal._processes.size).toBe(0);
	});

	it("a captured process wait cannot publish terminal status into a replacement registry entry", async () => {
		let settle!: (code: number) => void;
		const waiting = new Promise<number>((resolve) => {
			settle = resolve;
		});
		const process: ManagedProcess = {
			pid: 7,
			writeStdin: async () => {},
			closeStdin: async () => {},
			kill: vi.fn(),
			wait: () => waiting,
			exitCode: 143,
			exitSignal: 15,
		};
		const vm = vmWithKernel({});
		const internal = vm as unknown as {
			_trackProcess: (...args: unknown[]) => void;
			_processes: Map<number, unknown>;
		};
		internal._trackProcess(process, "node", [], true, new Set(), new Set());
		const pending = vm.process.wait(7);
		const replacement = {
			proc: { pid: 7 },
			exit: undefined,
			exitHandlers: new Set(),
		};
		internal._processes.set(7, replacement);
		settle(143);
		expect(await pending).toMatchObject({
			exitCode: 143,
			exitSignal: 15,
			signal: "SIGTERM",
		});
		expect(replacement.exit).toBeUndefined();
		expect(internal._processes.get(7)).toBe(replacement);
	});
});
