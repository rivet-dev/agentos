import { afterEach, describe, expect, it, vi } from "vitest";
import { SidecarRejectedError } from "../src/sidecar-errors.js";
import type { SidecarProcessSnapshotEntry } from "../src/sidecar/process-client.js";
import {
	type LocalCompatMount,
	SidecarKernelProxy,
	runManagedProcessToCompletion,
} from "../src/sidecar/rpc-client.js";

// Regression coverage for the SidecarKernelProxy tracking-collection leaks:
//   H6 - trackedProcesses / trackedProcessesById and the onStdout/onStderr
//        listener Sets were populated at spawn but never released on exit.
//   M8 - signalStates kept a per-pid entry forever (its sibling signalRefreshes
//        was already deleted on process_exited).
//   H7 - localMounts was never cleared on dispose().
// The proxy is exercised against a stub SidecarProcess so the test stays fast and
// deterministic without booting a real VM.

const session = { connectionId: "conn-1", sessionId: "sess-1" };
const vm = { vmId: "vm-test" };

afterEach(() => {
	vi.useRealTimers();
	vi.restoreAllMocks();
});

interface PumpEvent {
	ownership: { scope: string; vm_id: string };
	payload: Record<string, unknown>;
}

function createStubClient() {
	const queue: PumpEvent[] = [];
	const stdinWrites: unknown[] = [];
	let stdinCloseCount = 0;
	let notify: (() => void) | null = null;

	const client = {
		async execute() {
			return { pid: 4242 };
		},
		async getProcessSnapshot(): Promise<SidecarProcessSnapshotEntry[]> {
			return [];
		},
		async getSignalState() {
			return { handlers: new Map() };
		},
		async killProcess() {},
		async writeStdin(
			_session: unknown,
			_vm: unknown,
			_processId: unknown,
			data: unknown,
		) {
			stdinWrites.push(data);
		},
		async closeStdin() {
			stdinCloseCount += 1;
		},
		async disposeVm() {},
		async dispose() {},
		waitForEvent(
			_filter: unknown,
			_unused: unknown,
			options: { signal: AbortSignal },
		) {
			return new Promise<PumpEvent>((resolve, reject) => {
				const tryDeliver = () => {
					const event = queue.shift();
					if (event) {
						resolve(event);
						return true;
					}
					return false;
				};
				if (tryDeliver()) {
					return;
				}
				notify = () => {
					if (tryDeliver()) {
						notify = null;
					}
				};
				options.signal.addEventListener("abort", () =>
					reject(new Error("aborted")),
				);
			});
		},
	};

	const pushEvent = (event: PumpEvent) => {
		queue.push(event);
		notify?.();
	};

	return {
		client,
		pushEvent,
		stdinWrites,
		stdinCloseCount: () => stdinCloseCount,
	};
}

function createProxy(client: unknown, localMounts: LocalCompatMount[] = []) {
	const options = {
		client,
		session,
		vm,
		env: {},
		cwd: "/work",
		localMounts,
		sidecarMounts: [],
		commandGuestPaths: new Map<string, string>(),
		ownsClient: true,
	};
	return new SidecarKernelProxy(
		options as ConstructorParameters<typeof SidecarKernelProxy>[0],
	);
}

it("VM disposal preserves typed rejection after secondary cleanup and remains idempotent", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	const rejection = new SidecarRejectedError(1, {
		code: "timeout",
		message: "SQLite close is unconfirmed",
		limit_name: "reactor.shutdownDeadlineMs",
		configured_limit: 5_000,
		current_usage: null,
		requested: null,
		unit: "milliseconds",
		scope: "vm",
		vm_id: vm.vmId,
		session_generation: null,
		capability_id: null,
		operation: "vm.dispose",
		configuration_path: "limits.reactor.shutdownDeadlineMs",
		retryable: false,
		errno: "ETIMEDOUT",
	});
	const disposeVm = vi.spyOn(stub.client, "disposeVm").mockRejectedValue(rejection);
	const cleanup = vi.spyOn(stub.client, "dispose").mockRejectedValue(new Error("secondary cleanup"));
	const proxy = createProxy(stub.client);
	await expect(proxy.dispose()).rejects.toBe(rejection);
	expect(cleanup).toHaveBeenCalledOnce();
	expect(proxy.__trackingSizesForTest()).toEqual({
		trackedProcesses: 0,
		trackedProcessesById: 0,
		signalStates: 0,
		signalRefreshes: 0,
		localMounts: 0,
	});
	await expect(proxy.dispose()).resolves.toBeUndefined();
	expect(disposeVm).toHaveBeenCalledOnce();
});

it("exec timeout does not mistake a proxy's background failure for termination", async () => {
	vi.useFakeTimers();
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	stub.client.writeStdin = async () => {
		throw new Error("stdin transport failed");
	};
	const kill = vi.spyOn(stub.client, "killProcess");
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", [], { stdin: "input" });
		await vi.advanceTimersByTimeAsync(0);
		expect(proc.exitCode).toBeNull();
		const run = runManagedProcessToCompletion(
			proc,
			() => new Promise<void>(() => {}),
			Date.now(),
		);
		const assertion = expect(run).rejects.toThrow(/^termination_failed:/);
		await vi.runAllTimersAsync();
		await assertion;
		expect(kill).toHaveBeenCalledOnce();
	} finally {
		await proxy.dispose();
	}
});

it("spawn wait rejects a transport launch failure without inventing an exit", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	stub.client.execute = async () => {
		throw new Error("Execute transport failed");
	};
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		await expect(proc.wait()).rejects.toThrow(
			/^termination_failed:.*Execute transport failed/,
		);
		expect(proc.exitCode).toBeNull();
		expect(proxy.__trackingSizesForTest().trackedProcesses).toBe(1);
		await expect(
			proc.writeStdin("cannot enqueue after failure"),
		).rejects.toThrow(/^termination_failed:/);
		const tracked = (
			proxy as unknown as {
				trackedProcesses: Map<number, { pendingStdin: unknown[] }>;
			}
		).trackedProcesses.get(proc.pid);
		expect(tracked?.pendingStdin).toHaveLength(0);
		const kill = vi.spyOn(stub.client, "killProcess");
		proc.kill(9);
		await waitFor(() => kill.mock.calls.length > 0);
		expect(kill).toHaveBeenCalledOnce();
	} finally {
		await proxy.dispose();
	}
});

it("deterministic launch rejection releases tracking and exec skips termination cleanup", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	const rejection = new SidecarRejectedError(1, {
		code: "ENOENT",
		message: "command missing",
		limit_name: null,
		configured_limit: null,
		current_usage: null,
		requested: null,
		unit: null,
		scope: null,
		vm_id: null,
		session_generation: null,
		capability_id: null,
		operation: null,
		configuration_path: null,
		retryable: null,
		errno: "ENOENT",
	});
	stub.client.execute = async () => {
		throw rejection;
	};
	const kill = vi.spyOn(stub.client, "killProcess");
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("missing", []);
		proc.kill(9); // Rejection must also discard a signal queued before admission.
		await expect(
			runManagedProcessToCompletion(proc, () => proc.closeStdin(), undefined),
		).rejects.toBe(rejection);
		expect(proc.exitCode).toBeNull();
		expect(kill).not.toHaveBeenCalled();
		expect(proxy.__trackingSizesForTest().trackedProcesses).toBe(0);

		// The timeout may win just before Execute's deterministic rejection.
		// Do not spend the cleanup deadline waiting for a nonexistent exit.
		vi.useFakeTimers();
		let rejectLaunch!: (error: Error) => void;
		stub.client.execute = () =>
			new Promise((_resolve, reject) => {
				rejectLaunch = reject;
			});
		const late = proxy.spawn("missing", []);
		const timedOut = expect(
			runManagedProcessToCompletion(late, () => late.closeStdin(), Date.now()),
		).rejects.toThrow(/^timeout:/);
		await vi.advanceTimersByTimeAsync(0);
		rejectLaunch(rejection);
		await vi.advanceTimersByTimeAsync(0);
		await timedOut;
		expect(kill).not.toHaveBeenCalled();
		expect(late.exitCode).toBeNull();
		expect(vi.getTimerCount()).toBe(0);
	} finally {
		await proxy.dispose();
	}
});

it("missing snapshots fail wait but retain tracking for a later real exit", async () => {
	vi.useFakeTimers();
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		const assertion = expect(proc.wait()).rejects.toThrow(
			/^termination_failed:.*disappeared/,
		);
		await vi.advanceTimersByTimeAsync(700);
		await assertion;
		expect(proc.exitCode).toBeNull();
		expect(proxy.__trackingSizesForTest().trackedProcesses).toBe(1);
		expect(vi.getTimerCount()).toBe(0);
		stub.pushEvent({
			ownership: { scope: "vm", vm_id: vm.vmId },
			payload: {
				type: "process_exited",
				process_id: `proc-${proc.pid}`,
				exit_code: 7,
			},
		});
		await vi.advanceTimersByTimeAsync(0);
		await expect(proc.wait()).resolves.toBe(7);
		expect(proc.exitCode).toBe(7);
	} finally {
		await proxy.dispose();
	}
});

it("pump failure rejects wait and further spawn without declaring live processes exited", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	let rejectPump!: (error: Error) => void;
	stub.client.waitForEvent = () =>
		new Promise((_resolve, reject) => {
			rejectPump = reject;
		});
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		const assertion = expect(proc.wait()).rejects.toThrow(
			/^termination_failed:.*event stream failed/,
		);
		rejectPump(new Error("event stream failed"));
		await assertion;
		expect(proc.exitCode).toBeNull();
		expect(() => proxy.spawn("node", [])).toThrow(/^not_ready:/);
	} finally {
		await proxy.dispose();
	}
});

it("late stdin failure never overwrites a confirmed successful exit", async () => {
	vi.useFakeTimers();
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	let rejectWrite!: (error: Error) => void;
	stub.client.writeStdin = () =>
		new Promise((_resolve, reject) => {
			rejectWrite = reject;
		});
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", [], { streamStdin: true });
		const assertion = expect(proc.writeStdin("input")).rejects.toThrow(
			"late stdin failure",
		);
		await vi.advanceTimersByTimeAsync(0);
		stub.pushEvent({
			ownership: { scope: "vm", vm_id: vm.vmId },
			payload: {
				type: "process_exited",
				process_id: `proc-${proc.pid}`,
				exit_code: 0,
			},
		});
		await vi.advanceTimersByTimeAsync(0);
		rejectWrite(new Error("late stdin failure"));
		await assertion;
		await expect(proc.wait()).resolves.toBe(0);
		expect(proc.exitCode).toBe(0);
	} finally {
		await proxy.dispose();
	}
});

it("ordinary kill rejection is observed by wait instead of an unhandled promise", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	stub.client.killProcess = async () => {
		throw new Error("signal transport failed");
	};
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		const assertion = expect(proc.wait()).rejects.toThrow(
			/^termination_failed:.*signal transport failed/,
		);
		proc.kill(9);
		await assertion;
		expect(proc.exitCode).toBeNull();
	} finally {
		await proxy.dispose();
	}
});

it("VM disposal rejects pending wait rather than manufacturing signal exit 143", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	const proxy = createProxy(stub.client);
	const proc = proxy.spawn("node", []);
	await waitFor(() => stub.stdinCloseCount() > 0);
	const assertion = expect(proc.wait()).rejects.toThrow(
		/^termination_failed:.*disposed/,
	);
	await proxy.dispose();
	await assertion;
	expect(proc.exitCode).toBeNull();
	expect(() => proxy.spawn("node", [])).toThrow(/^not_ready:/);
});

it("a real exit wakes wait even while the snapshot RPC is stalled", async () => {
	vi.useFakeTimers();
	const stub = createStubClient();
	stub.client.getProcessSnapshot = () => new Promise(() => {});
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		const result = proc.wait();
		await vi.advanceTimersByTimeAsync(100);
		stub.pushEvent({
			ownership: { scope: "vm", vm_id: vm.vmId },
			payload: {
				type: "process_exited",
				process_id: `proc-${proc.pid}`,
				exit_code: 23,
			},
		});
		await vi.advanceTimersByTimeAsync(0);
		await expect(result).resolves.toBe(23);
		expect(vi.getTimerCount()).toBe(0);
	} finally {
		await proxy.dispose();
	}
});

it("disposal wakes wait before a stalled teardown signal completes", async () => {
	vi.useFakeTimers();
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	stub.client.getProcessSnapshot = () => new Promise(() => {});
	let acknowledgeKill!: () => void;
	stub.client.killProcess = () =>
		new Promise((resolve) => {
			acknowledgeKill = resolve;
		});
	const proxy = createProxy(stub.client);
	const proc = proxy.spawn("node", []);
	const assertion = expect(proc.wait()).rejects.toThrow(
		/^termination_failed:.*disposed/,
	);
	await vi.advanceTimersByTimeAsync(100);
	const disposal = proxy.dispose();
	try {
		await assertion;
		expect(proc.exitCode).toBeNull();
	} finally {
		acknowledgeKill();
		await disposal;
	}
});

it("an exited snapshot without an exit code cannot manufacture success", async () => {
	vi.useFakeTimers();
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		stub.client.getProcessSnapshot = async () => [
			{
				processId: `proc-${proc.pid}`,
				pid: 42,
				ppid: 0,
				pgid: 42,
				sid: 42,
				driver: "node",
				cwd: "/work",
				command: "node",
				args: [],
				status: "exited",
				exitCode: null,
			},
		];
		const assertion = expect(proc.wait()).rejects.toThrow(
			/exited snapshot has no exit status/,
		);
		await vi.advanceTimersByTimeAsync(100);
		await assertion;
		expect(proc.exitCode).toBeNull();
	} finally {
		await proxy.dispose();
	}
});

it("disposal before launch admission never sends Execute", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const stub = createStubClient();
	const execute = vi.spyOn(stub.client, "execute");
	const proxy = createProxy(stub.client);
	const proc = proxy.spawn("node", []);
	const wait = expect(proc.wait()).rejects.toThrow(
		/^(not_ready|termination_failed):/,
	);
	await proxy.dispose();
	await wait;
	expect(execute).not.toHaveBeenCalled();
	expect(proc.exitCode).toBeNull();
});

it("exec timeout accepts a real proxy exit event as termination proof", async () => {
	vi.useFakeTimers();
	const stub = createStubClient();
	stub.client.killProcess = async () => {
		stub.pushEvent({
			ownership: { scope: "vm", vm_id: vm.vmId },
			payload: {
				type: "process_exited",
				process_id: "proc-1000000",
				exit_code: 137,
			},
		});
	};
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		const run = runManagedProcessToCompletion(
			proc,
			() => new Promise<void>(() => {}),
			Date.now(),
		);
		const assertion = expect(run).rejects.toThrow(/^timeout:.*was stopped$/);
		await vi.runAllTimersAsync();
		await assertion;
	} finally {
		await proxy.dispose();
	}
});

it("exec timeout surfaces a real proxy kill rejection as termination_failed", async () => {
	vi.useFakeTimers();
	const stub = createStubClient();
	stub.client.killProcess = async () => {
		throw new Error("kill transport failed");
	};
	const proxy = createProxy(stub.client);
	try {
		const proc = proxy.spawn("node", []);
		const run = runManagedProcessToCompletion(
			proc,
			() => new Promise<void>(() => {}),
			Date.now(),
		);
		const assertion = expect(run).rejects.toThrow(
			/^termination_failed:.*kill transport failed/,
		);
		await vi.runAllTimersAsync();
		await assertion;
	} finally {
		await proxy.dispose();
	}
});

async function waitFor(predicate: () => boolean, timeoutMs = 500) {
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		if (predicate()) {
			return;
		}
		await new Promise((resolve) => setTimeout(resolve, 5));
	}
}

describe("SidecarKernelProxy tracking-collection cleanup", () => {
	it("forwards initial stdin and closes non-streaming process input", async () => {
		const stub = createStubClient();
		const proxy = createProxy(stub.client);

		proxy.spawn("node", ["script.js"], {
			stdin: "initial input",
			streamStdin: false,
		});

		await waitFor(() => stub.stdinCloseCount() === 1);
		expect(stub.stdinWrites).toEqual(["initial input"]);
		expect(stub.stdinCloseCount()).toBe(1);
		await proxy.dispose();
	});

	it("releases tracked process + signal state + listeners when a process exits", async () => {
		const { client, pushEvent } = createStubClient();
		const proxy = createProxy(client);

		const proc = proxy.spawn("node", ["script.js"], {
			onStdout: () => {},
			onStderr: () => {},
		});

		// Populate signalStates the same way the kernel does (getSignalState ->
		// refreshSignalState), so we can prove it is released on exit.
		proxy.getSignalState(proc.pid);
		await proxy.__awaitSignalRefreshesForTest();

		const entry = proxy.__trackedEntryForTest(proc.pid);
		expect(entry).toBeDefined();
		expect(entry?.onStdout.size).toBe(1);
		expect(entry?.onStderr.size).toBe(1);

		const before = proxy.__trackingSizesForTest();
		expect(before.trackedProcesses).toBe(1);
		expect(before.trackedProcessesById).toBe(1);
		expect(before.signalStates).toBe(1);

		// Drive the real event-pump exit path (the sibling signalRefreshes delete
		// already lives here; signalStates must be released alongside it).
		pushEvent({
			ownership: { scope: "vm", vm_id: vm.vmId },
			payload: {
				type: "process_exited",
				process_id: `proc-${proc.pid}`,
				exit_code: 0,
			},
		});

		await waitFor(() => proxy.__trackingSizesForTest().trackedProcesses === 0);

		const after = proxy.__trackingSizesForTest();
		expect(after.trackedProcesses).toBe(0);
		expect(after.trackedProcessesById).toBe(0);
		expect(after.signalStates).toBe(0);
		// The listener Sets on the (now untracked) entry must be emptied too.
		expect(entry?.onStdout.size).toBe(0);
		expect(entry?.onStderr.size).toBe(0);

		await proxy.dispose();
	});

	it("clears all tracking state and local mounts on dispose", async () => {
		const { client } = createStubClient();
		const localMount: LocalCompatMount = {
			path: "/mnt/data",
			fs: {} as LocalCompatMount["fs"],
			readOnly: false,
		};
		const proxy = createProxy(client, [localMount]);

		const proc = proxy.spawn("node", ["server.js"], {
			onStdout: () => {},
		});
		proxy.getSignalState(proc.pid);
		await proxy.__awaitSignalRefreshesForTest();

		const before = proxy.__trackingSizesForTest();
		expect(before.trackedProcesses).toBe(1);
		expect(before.localMounts).toBe(1);
		expect(before.signalStates).toBe(1);

		// Dispose with a still-live process: every collection must end up empty.
		await proxy.dispose();

		const after = proxy.__trackingSizesForTest();
		expect(after.trackedProcesses).toBe(0);
		expect(after.trackedProcessesById).toBe(0);
		expect(after.signalStates).toBe(0);
		expect(after.signalRefreshes).toBe(0);
		expect(after.localMounts).toBe(0);
	});
});
