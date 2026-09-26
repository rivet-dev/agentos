import { afterEach, expect, test, vi } from "vitest";
import type { ManagedProcess } from "../src/runtime-compat.js";
import { runManagedProcessToCompletion } from "../src/sidecar/rpc-client.js";

afterEach(() => {
	vi.useRealTimers();
});

function processThatExitsWhenKilled(): {
	process: ManagedProcess;
	killSignals: number[];
} {
	let resolveExit!: (code: number) => void;
	const exit = new Promise<number>((resolve) => {
		resolveExit = resolve;
	});
	const killSignals: number[] = [];
	return {
		process: {
			pid: 42,
			writeStdin: async () => {},
			closeStdin: async () => {},
			kill: (signal = 15) => {
				killSignals.push(signal);
				resolveExit(128 + signal);
			},
			wait: () => exit,
			exitCode: null,
		},
		killSignals,
	};
}

test("execution deadline includes blocked stdin and confirms exit after SIGKILL", async () => {
	vi.useFakeTimers();
	const { process, killSignals } = processThatExitsWhenKilled();
	const run = runManagedProcessToCompletion(
		process,
		() => new Promise<void>(() => {}),
		Date.now(),
	);
	const assertion = expect(run).rejects.toThrow(/^timeout:.*was stopped$/);
	await vi.runAllTimersAsync();
	await assertion;
	expect(killSignals).toEqual([9]);
});

test("execution deadline reports unconfirmed termination separately", async () => {
	vi.useFakeTimers();
	const process: ManagedProcess = {
		pid: 43,
		writeStdin: async () => {},
		closeStdin: async () => {},
		kill: () => {},
		wait: () => new Promise<number>(() => {}),
		exitCode: null,
	};
	const run = runManagedProcessToCompletion(
		process,
		async () => {},
		Date.now(),
	);
	const assertion = expect(run).rejects.toThrow(/^termination_failed:/);
	await vi.runAllTimersAsync();
	await assertion;
});

test("synchronous kill rejection is reported as unconfirmed termination", async () => {
	vi.useFakeTimers();
	const { process } = processThatExitsWhenKilled();
	process.kill = () => {
		throw new Error("signal rejected");
	};
	const run = runManagedProcessToCompletion(
		process,
		async () => {},
		Date.now(),
	);
	const assertion = expect(run).rejects.toThrow(
		/^termination_failed:.*signal rejected/,
	);
	await vi.runAllTimersAsync();
	await assertion;
});

test("stdin failure stops the guest before surfacing the original error", async () => {
	const { process, killSignals } = processThatExitsWhenKilled();
	await expect(
		runManagedProcessToCompletion(
			process,
			async () => {
				throw new Error("stdin failed");
			},
			undefined,
		),
	).rejects.toThrow("stdin failed");
	expect(killSignals).toEqual([9]);
});
