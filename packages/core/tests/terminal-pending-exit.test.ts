import { describe, expect, it, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";

// Actual AgentOs terminal methods; the kernel handle's launch/exit are injected.
function fixture() {
	let settle!: (code: number) => void;
	let exitSignal: number | null = null;
	const waiting = new Promise<number>((resolve) => {
		settle = resolve;
	});
	const handle = {
		pid: 42,
		kill: vi.fn(),
		wait: () => waiting,
		write: vi.fn(),
		resize: vi.fn(),
		onData: null,
		get exitSignal() {
			return exitSignal;
		},
	};
	const vm = Reflect.construct(AgentOs, [
		{ openShell: () => handle },
		{},
		[],
		[],
		{},
		{},
		{ onEvent: () => () => {}, onClose: () => () => {} },
		{},
		{},
	]) as AgentOs;
	return {
		vm,
		handle,
		finish(code: number, signal: number | null) {
			exitSignal = signal;
			settle(code);
		},
	};
}

describe("terminal pending exit ownership", () => {
	it.each([
		[0, null],
		[137, 9],
	] as const)("close acknowledges without claiming exit (%i,%s)", async (code, signal) => {
		const { vm, handle, finish } = fixture();
		const shell = vm.terminal.open();
		vm.terminal.close(shell.shellId);
		expect(handle.kill).toHaveBeenCalledTimes(1);
		const pending = (
			vm as unknown as {
				_pendingShellExitPromises: Map<string, Promise<unknown>>;
			}
		)._pendingShellExitPromises;
		expect(pending.size).toBe(1);
		expect(() => vm.terminal.write(shell.shellId, "after-close")).toThrow(
			"Shell not found",
		);
		let settled = false;
		const waiting = Promise.resolve()
			.then(() => vm.terminal.waitExit(shell.shellId))
			.then(
				(value) => {
					settled = true;
					return { value };
				},
				(error) => {
					settled = true;
					return { error };
				},
			);
		await Promise.resolve();
		await Promise.resolve();
		const settledBeforeExit = settled;
		finish(code, signal);
		const result = await waiting;
		expect(
			settledBeforeExit,
			"close lost pending waiter or invented an exit",
		).toBe(false);
		expect(result).toEqual({
			value: {
				shellId: shell.shellId,
				exitCode: code,
				...(signal == null ? {} : { exitSignal: signal }),
			},
		});
		expect(await vm.terminal.waitExit(shell.shellId)).toEqual(
			"value" in result ? result.value : undefined,
		);
		expect(pending.size).toBe(0);
	});
});
