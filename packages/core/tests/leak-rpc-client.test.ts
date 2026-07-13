import { describe, expect, it } from "vitest";
import {
	type LocalCompatMount,
	NativeSidecarKernelProxy,
} from "../src/sidecar/rpc-client.js";

// Regression coverage for the NativeSidecarKernelProxy tracking-collection leaks:
//   H6 - trackedProcesses / trackedProcessesById and the onStdout/onStderr
//        listener Sets were populated at spawn but never released on exit.
//   H7 - localMounts was never cleared on dispose().
// The proxy is exercised against a stub SidecarProcess so the test stays fast and
// deterministic without booting a real VM.

const session = { connectionId: "conn-1", sessionId: "sess-1" };
const vm = { vmId: "vm-test" };

interface PumpEvent {
	ownership: { scope: string; vm_id: string };
	payload: Record<string, unknown>;
}

function createStubClient() {
	const queue: PumpEvent[] = [];
	let notify: (() => void) | null = null;
	let rejectPump: ((error: Error) => void) | null = null;
	let processId: string | null = null;

	const client = {
		async execute() {
			processId = "sidecar-process-1";
			return { processId, pid: 4242 };
		},
		async getProcessSnapshot() {
			return [];
		},
		async killProcess() {},
		async writeStdin() {},
		async closeStdin() {},
		async disposeVm() {},
		async dispose() {},
		waitForEvent(
			_filter: unknown,
			_unused: unknown,
			options: { signal: AbortSignal },
		) {
			return new Promise<PumpEvent>((resolve, reject) => {
				rejectPump = reject;
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
	const failPump = (error: Error) => rejectPump?.(error);

	return { client, pushEvent, failPump, processId: () => processId };
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
	return new NativeSidecarKernelProxy(
		options as ConstructorParameters<typeof NativeSidecarKernelProxy>[0],
	);
}

async function waitFor(predicate: () => boolean, timeoutMs = 500) {
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		if (predicate()) {
			return;
		}
		await new Promise((resolve) => setTimeout(resolve, 5));
	}
}

describe("NativeSidecarKernelProxy tracking-collection cleanup", () => {
	it("releases tracked process routes and listeners when a process exits", async () => {
		const { client, pushEvent, processId } = createStubClient();
		const proxy = createProxy(client);

		const proc = await proxy.spawn("node", ["script.js"], {
			onStdout: () => {},
			onStderr: () => {},
		});

		const entry = proxy.__trackedEntryForTest(proc.pid);
		expect(entry).toBeDefined();
		expect(entry?.onStdout.size).toBe(1);
		expect(entry?.onStderr.size).toBe(1);

		const before = proxy.__trackingSizesForTest();
		expect(before.trackedProcesses).toBe(1);
		expect(before.trackedProcessesById).toBe(1);

		// Drive the real event-pump exit path.
		pushEvent({
			ownership: { scope: "vm", vm_id: vm.vmId },
			payload: {
				type: "process_exited",
				process_id: processId(),
				exit_code: 0,
			},
		});

		await waitFor(() => proxy.__trackingSizesForTest().trackedProcesses === 0);

		const after = proxy.__trackingSizesForTest();
		expect(after.trackedProcesses).toBe(0);
		expect(after.trackedProcessesById).toBe(0);
		// The listener Sets on the (now untracked) entry must be emptied too.
		expect(entry?.onStdout.size).toBe(0);
		expect(entry?.onStderr.size).toBe(0);

		await proxy.dispose();
	});

	it("rejects wait when the authoritative event stream fails", async () => {
		const { client, failPump } = createStubClient();
		const proxy = createProxy(client);
		const proc = await proxy.spawn("node", ["server.js"]);
		const waiting = proc.wait();

		failPump(new Error("sidecar transport closed"));

		await expect(waiting).rejects.toThrow("sidecar transport closed");
		expect(proc.exitCode).toBeNull();
		await waitFor(() => proxy.__trackingSizesForTest().trackedProcesses === 0);
		expect(proxy.__trackingSizesForTest().trackedProcesses).toBe(0);
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

		const proc = await proxy.spawn("node", ["server.js"], {
			onStdout: () => {},
		});
		const before = proxy.__trackingSizesForTest();
		expect(before.trackedProcesses).toBe(1);
		expect(before.localMounts).toBe(1);

		// Dispose with a still-live process: every collection must end up empty.
		await proxy.dispose();

		const after = proxy.__trackingSizesForTest();
		expect(after.trackedProcesses).toBe(0);
		expect(after.trackedProcessesById).toBe(0);
		expect(after.localMounts).toBe(0);
	});

	it("surfaces sidecar teardown failures after clearing local state", async () => {
		const { client } = createStubClient();
		let disposedClient = false;
		client.disposeVm = async () => {
			throw new Error("dispose VM failed");
		};
		client.dispose = async () => {
			disposedClient = true;
		};
		const proxy = createProxy(client, [
			{
				path: "/mnt/data",
				fs: {} as LocalCompatMount["fs"],
				readOnly: false,
			},
		]);

		await expect(proxy.dispose()).rejects.toThrow(
			"failed to dispose sidecar VM",
		);
		expect(disposedClient).toBe(true);
		expect(proxy.__trackingSizesForTest()).toEqual({
			trackedProcesses: 0,
			trackedProcessesById: 0,
			localMounts: 0,
		});
	});
});
