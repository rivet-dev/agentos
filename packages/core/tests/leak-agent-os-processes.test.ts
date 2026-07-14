import { describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";

/**
 * Regression test for the `_processes` Map leak (H5).
 *
 * Completed PID/exit-code correlation is needed for the synchronous public
 * process helpers, but retaining every ManagedProcess and listener set keeps the
 * entire sidecar proxy graph alive. This builds a minimal AgentOs over mocked
 * dependencies to prove completed routes are lightweight and bounded.
 */

interface MockProc {
	pid: number;
	wait: () => Promise<number>;
	exitCode: number | null;
	kill: () => Promise<void>;
}

/** Build an AgentOs instance backed by mocks, bypassing the real sidecar/VM. */
function makeAgentOs(processRouteRetention = 1_024): {
	vm: AgentOs;
	processes: Map<number, unknown>;
} {
	const noop = () => {};
	const kernelMock = { dispose: async () => {} };
	const sidecarClientMock = { onEvent: () => noop };

	// The constructor is private at the type level but callable at runtime; it
	// runs the class field initializers (including `_processes = new Map()`).
	const vm = new (AgentOs as unknown as new (...args: unknown[]) => AgentOs)(
		kernelMock,
		{},
		{},
		{},
		sidecarClientMock,
		{},
		{ processRouteRetention },
	);
	// `_cronManager` is assigned post-construction in the real factory; supply a
	// stub so `dispose()` can run.
	(vm as unknown as { _cronManager: { dispose: () => void } })._cronManager = {
		dispose: noop,
	};

	const processes = (vm as unknown as { _processes: Map<number, unknown> })
		._processes;
	return { vm, processes };
}

function makeProc(pid: number): {
	proc: MockProc;
	resolveWait: (code: number) => void;
	rejectWait: (error: Error) => void;
} {
	let resolveWait!: (code: number) => void;
	let rejectWait!: (error: Error) => void;
	const waitPromise = new Promise<number>((resolve, reject) => {
		resolveWait = resolve;
		rejectWait = reject;
	});
	const proc: MockProc = {
		pid,
		wait: () => waitPromise,
		exitCode: null,
		kill: async () => {},
	};
	return { proc, resolveWait, rejectWait };
}

async function track(vm: AgentOs, proc: MockProc): Promise<void> {
	await (
		vm as unknown as {
			_trackProcess: (
				p: MockProc,
				a: Set<unknown>,
				b: Set<unknown>,
				c: Set<unknown>,
			) => Promise<{ pid: number }>;
		}
	)._trackProcess(proc, new Set(), new Set(), new Set());
}

describe("AgentOs _processes leak (H5)", () => {
	test("exited process retains only lightweight query correlation", async () => {
		const { vm, processes } = makeAgentOs();
		const { proc, resolveWait } = makeProc(101);

		await track(vm, proc);
		expect(processes.size).toBe(1);

		resolveWait(0);
		await Promise.resolve();

		expect(processes.size).toBe(1);
		expect(processes.get(101)).toEqual({ state: "exited", exitCode: 0 });
		await expect(vm.waitProcess(101)).resolves.toBe(0);
		const lateExit = vi.fn();
		vm.onProcessExit(101, lateExit);
		expect(lateExit).toHaveBeenCalledWith(0);
		expect(() => vm.onProcessStdout(101, vi.fn())).not.toThrow();
		expect(() => vm.onProcessStderr(101, vi.fn())).not.toThrow();
	});

	test("completed correlation obeys the sidecar-advertised retention", async () => {
		const { vm, processes } = makeAgentOs();

		for (let pid = 1; pid <= 1_025; pid++) {
			const { proc, resolveWait } = makeProc(pid);
			await track(vm, proc);
			resolveWait(0);
			await Promise.resolve();
		}

		expect(processes.size).toBe(1_024);
		expect(processes.has(1)).toBe(false);
		expect(processes.get(1_025)).toEqual({ state: "exited", exitCode: 0 });
	});

	test("evicts terminal routes by completion order", async () => {
		const { vm, processes } = makeAgentOs(2);
		const first = makeProc(10);
		const second = makeProc(20);
		const third = makeProc(30);
		await track(vm, first.proc);
		await track(vm, second.proc);
		await track(vm, third.proc);

		second.resolveWait(0);
		await Promise.resolve();
		first.resolveWait(0);
		await Promise.resolve();
		third.resolveWait(0);
		await Promise.resolve();

		expect([...processes.keys()]).toEqual([10, 30]);
	});

	test("zero retention still settles in-flight waiters and exit handlers", async () => {
		const { vm, processes } = makeAgentOs(0);
		const { proc, resolveWait } = makeProc(404);
		await track(vm, proc);
		const exitHandler = vi.fn();
		vm.onProcessExit(404, exitHandler);
		const wait = vm.waitProcess(404);

		resolveWait(7);
		await expect(wait).resolves.toBe(7);
		await Promise.resolve();

		expect(exitHandler).toHaveBeenCalledWith(7);
		expect(processes.size).toBe(0);
	});

	test("failed process retains a lightweight typed failure for late waiters", async () => {
		const { vm, processes } = makeAgentOs();
		const { proc, rejectWait } = makeProc(303);
		const routeError = Object.assign(new Error("event stream closed"), {
			code: "event_stream_closed",
		});

		const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
		try {
			await track(vm, proc);
			const firstWait = vm.waitProcess(303);
			rejectWait(routeError);
			await expect(firstWait).rejects.toBe(routeError);
			await Promise.resolve();

			expect(processes.get(303)).toEqual({
				state: "failed",
				error: routeError,
			});
			await expect(vm.waitProcess(303)).rejects.toBe(routeError);
		} finally {
			errorLog.mockRestore();
		}
	});

	test("dispose() clears _processes for still-running processes", async () => {
		const { vm, processes } = makeAgentOs();
		const { proc } = makeProc(202);

		// Track a process whose wait() never resolves (still running at dispose).
		await track(vm, proc);
		expect(processes.size).toBe(1);

		await vm.dispose();

		expect(processes.size).toBe(0);
	});
});
