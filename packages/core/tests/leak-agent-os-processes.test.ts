import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

/**
 * Regression test for the `_processes` Map leak (H5).
 *
 * The leak was that `dispose()` cleared `_shells`/`_acpTerminals` but never
 * `_processes`, so the process table outlived the VM. The fix clears it in
 * dispose(). Exited processes are deliberately RETAINED until then — the public
 * API (getProcess/listProcesses/stopProcess, see process-management.test.ts)
 * requires querying processes after they exit, so deleting on exit is wrong.
 * This builds a minimal AgentOs over mocked dependencies to drive the lifecycle.
 */

interface MockProc {
	pid: number;
	wait: () => Promise<number>;
	exitCode: number | null;
	kill: () => void;
}

/** Build an AgentOs instance backed by mocks, bypassing the real sidecar/VM. */
function makeAgentOs(): {
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
		[],
		new Map(),
		[],
		{},
		{},
		sidecarClientMock,
		{},
		{},
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
} {
	let resolveWait!: (code: number) => void;
	const waitPromise = new Promise<number>((r) => {
		resolveWait = r;
	});
	const proc: MockProc = {
		pid,
		wait: () => waitPromise,
		exitCode: null,
		kill: () => {},
	};
	return { proc, resolveWait };
}

function track(vm: AgentOs, proc: MockProc): void {
	(
		vm as unknown as {
			_trackProcess: (
				p: MockProc,
				command: string,
				args: string[],
				a: Set<unknown>,
				b: Set<unknown>,
				c: Set<unknown>,
			) => { pid: number };
		}
	)._trackProcess(proc, "cmd", [], new Set(), new Set(), new Set());
}

describe("AgentOs _processes leak (H5)", () => {
	test("exited process is retained in _processes (queryable until dispose)", async () => {
		const { vm, processes } = makeAgentOs();
		const { proc, resolveWait } = makeProc(101);

		track(vm, proc);
		expect(processes.size).toBe(1);

		// Process exits: the entry must REMAIN so getProcess/listProcesses still
		// report it (running:false, exitCode set). Deleting here is the bug that
		// broke process-management.test.ts.
		resolveWait(0);
		await new Promise((r) => setTimeout(r, 0));

		expect(processes.size).toBe(1);
	});

	test("dispose() clears _processes for still-running processes", async () => {
		const { vm, processes } = makeAgentOs();
		const { proc } = makeProc(202);

		// Track a process whose wait() never resolves (still running at dispose).
		track(vm, proc);
		expect(processes.size).toBe(1);

		await vm.dispose();

		expect(processes.size).toBe(0);
	});
});
