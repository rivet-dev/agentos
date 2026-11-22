import { describe, expect, it, vi } from "vitest";
import {
	buildProcessForest,
	type ProcessForestRecord,
} from "../src/process-forest.js";
import { SidecarKernelProxy } from "../src/sidecar/rpc-client.js";
import { SidecarRejectedError } from "../src/sidecar-errors.js";

describe("process-tree identity", () => {
	it("uses authoritative exited state while the tracked exit event is still pending", async () => {
		const tracked = {
			processId: "tracked-root",
			pid: 1_000_000,
			driver: "node",
			command: "node",
			args: [],
			cwd: "/",
			exitCode: null as number | null,
			startTime: 1,
			exitTime: null,
		};
		const native = {
			processId: tracked.processId,
			pid: 42,
			ppid: 0,
			pgid: 42,
			sid: 42,
			driver: "node",
			command: "node",
			args: [],
			cwd: "/",
			status: "exited",
			exitCode: 23,
		};
		const proxy = Object.create(
			SidecarKernelProxy.prototype,
		) as SidecarKernelProxy;
		Object.assign(proxy, {
			client: { getProcessSnapshot: async () => [native] },
			session: {},
			vm: {},
			processSnapshotRefresh: null,
			sidecarProcessSnapshot: [],
			trackedProcesses: new Map([[tracked.pid, tracked]]),
			trackedProcessesById: new Map([[tracked.processId, tracked]]),
			observedProcessStartTimes: new Map(),
			processes: new Map(),
		});
		const [record] = await proxy.currentProcessTopology();
		expect(record.info.status).toBe("exited");
		expect(record.info.exitCode).toBe(23);
		expect(record.info.pid).toBe(tracked.pid);
		// Conversely, an earlier snapshot must never resurrect a confirmed exit.
		tracked.exitCode = 23;
		native.status = "running";
		expect((await proxy.currentProcessTopology())[0].info.status).toBe(
			"exited",
		);
	});

	it("retires observed start-time metadata when processes leave the current snapshot", async () => {
		let pid = 1;
		const starts = new Map<string, number>();
		const proxy = Object.create(
			SidecarKernelProxy.prototype,
		) as SidecarKernelProxy;
		Object.assign(proxy, {
			client: {
				getProcessSnapshot: async () => [
					{
						processId: `child-${pid}`,
						pid,
						ppid: 0,
						pgid: pid,
						sid: pid,
						driver: "node",
						command: "node",
						args: [],
						cwd: "/",
						status: "running",
						exitCode: null,
					},
				],
			},
			session: {},
			vm: {},
			processSnapshotRefresh: null,
			sidecarProcessSnapshot: [],
			trackedProcesses: new Map(),
			trackedProcessesById: new Map(),
			observedProcessStartTimes: starts,
			processes: new Map(),
		});
		for (pid = 1; pid <= 4; pid++) {
			const [record] = await proxy.currentProcessTopology();
			expect(starts.size).toBe(1);
			expect(starts.get(`child-${pid}:${pid}`)).toBe(record.info.startTime);
			expect((await proxy.currentProcessTopology())[0].info.startTime).toBe(
				record.info.startTime,
			);
		}
	});

	it("links by raw topology when tracked and guest display PIDs collide", () => {
		type Info = { pid: number; command: string };
		type Node = Info & { children: Node[] };
		const records: ProcessForestRecord<Info>[] = [
			{
				key: "kernel:7",
				parentKey: null,
				info: { pid: 1_000_000, command: "tracked" },
			},
			{
				key: "kernel:1000000",
				parentKey: "kernel:7",
				info: { pid: 1_000_000, command: "guest" },
			},
			{
				key: "kernel:8",
				parentKey: "kernel:1000000",
				info: { pid: 8, command: "grandchild" },
			},
			{
				key: "tracked:8",
				parentKey: null,
				info: { pid: 8, command: "pending tracked" },
			},
		];
		const roots = buildProcessForest<Info, Node>(records, (info, children) => ({
			...info,
			children,
		}));
		expect(roots.map((root) => root.command)).toEqual([
			"tracked",
			"pending tracked",
		]);
		expect(roots[0].children.map((child) => child.command)).toEqual(["guest"]);
		expect(roots[0].children[0].children[0].command).toBe("grandchild");
		expect(roots[1].children).toEqual([]);
	});

	it("preserves an untracked running row and a colliding tracked fallback", async () => {
		const client = {
			getProcessSnapshot: async () => [
				{
					processId: "root/child",
					pid: 1_000_000,
					ppid: 7,
					pgid: 7,
					sid: 7,
					driver: "wasm",
					command: "guest",
					args: [],
					cwd: "/",
					status: "running",
					exitCode: null,
				},
			],
		};
		const tracked = {
			processId: "tracked-root",
			pid: 1_000_000,
			driver: "node",
			command: "tracked",
			args: [],
			cwd: "/",
			exitCode: null,
			startTime: 1,
			exitTime: null,
		};
		// No VM or event pump is required: exercise the real snapshot adapter with
		// its bounded host-side registries populated directly.
		const proxy = Object.create(
			SidecarKernelProxy.prototype,
		) as SidecarKernelProxy;
		Object.assign(proxy, {
			client,
			session: {},
			vm: {},
			processSnapshotRefresh: null,
			sidecarProcessSnapshot: [],
			trackedProcesses: new Map([[tracked.pid, tracked]]),
			trackedProcessesById: new Map([[tracked.processId, tracked]]),
			observedProcessStartTimes: new Map(),
			processes: new Map(),
		});
		const records = await proxy.currentProcessTopology();
		expect(records).toHaveLength(2);
		expect(records.map((record) => record.info.pid)).toEqual([
			1_000_000, 1_000_000,
		]);
		expect(records.map((record) => record.info.status)).toEqual([
			"running",
			"running",
		]);
		expect(records.map((record) => record.key)).toEqual([
			"kernel:1000000",
			"tracked:1000000",
		]);
		expect(proxy.processes.get(1_000_000)?.command).toBe("tracked");
	});

	it("awaits a coalesced current snapshot instead of returning a stale running process", async () => {
		const running = {
			processId: "shell",
			pid: 42,
			ppid: 0,
			pgid: 42,
			sid: 42,
			driver: "wasm",
			command: "sh",
			args: [],
			cwd: "/",
			status: "running",
			exitCode: null,
		};
		let resolveSnapshot!: (value: (typeof running)[]) => void;
		const response = new Promise<(typeof running)[]>((resolve) => {
			resolveSnapshot = resolve;
		});
		const getProcessSnapshot = vi.fn(() => response);
		const proxy = Object.create(
			SidecarKernelProxy.prototype,
		) as SidecarKernelProxy;
		Object.assign(proxy, {
			client: { getProcessSnapshot },
			session: {},
			vm: {},
			processSnapshotRefresh: null,
			sidecarProcessSnapshot: [running],
			trackedProcesses: new Map(),
			trackedProcessesById: new Map(),
			observedProcessStartTimes: new Map(),
			processes: new Map(),
		});

		const first = proxy.currentProcessTopology();
		const second = proxy.currentProcessTopology();
		let settled = false;
		void first.then(() => {
			settled = true;
		});
		await Promise.resolve();
		expect(settled).toBe(false);
		expect(getProcessSnapshot).toHaveBeenCalledTimes(1);

		resolveSnapshot([{ ...running, status: "exited", exitCode: 0 }]);
		expect((await first)[0].info.status).toBe("exited");
		expect((await second)[0].info.status).toBe("exited");
		expect(getProcessSnapshot).toHaveBeenCalledTimes(1);

		getProcessSnapshot.mockRejectedValueOnce(new Error("snapshot unavailable"));
		await expect(proxy.currentProcessTopology()).rejects.toThrow(
			"snapshot unavailable",
		);
	});
});

it.each([
	{
		code: "ERR_AGENTOS_VM_LIFECYCLE_CONFLICT",
		retryable: true,
		errno: "EAGAIN",
	},
	{
		code: "ERR_AGENTOS_VM_LIFECYCLE_CONFLICT",
		retryable: false,
		errno: "EAGAIN",
	},
	{
		code: "ERR_AGENTOS_OTHER_FAILURE",
		retryable: true,
		errno: "EAGAIN",
	},
	{
		code: "ERR_AGENTOS_VM_LIFECYCLE_CONFLICT",
		retryable: true,
		errno: "EIO",
	},
])("event-driven process wait preserves typed failures without polling ($code,$retryable,$errno)", async ({
	code,
	retryable,
	errno,
}) => {
	vi.useFakeTimers();
	try {
		const blocked = new SidecarRejectedError(93, {
			code,
			message: "VM lifecycle is Active",
			retryable,
			errno,
			operation: "vm.lifecycleAdmission",
			limit_name: null,
			configured_limit: null,
			current_usage: null,
			requested: 1,
			unit: "requests",
			scope: "vm",
			vm_id: "vm",
			session_generation: null,
			capability_id: null,
			configuration_path: null,
		});
		const entry = {
			pid: 1000000,
			processId: "owned",
			started: true,
			exitCode: null as number | null,
			waitError: null as unknown,
			waitPromise: null,
			wakeWait: null as (() => void) | null,
			hostExitObservedAt: null,
			confirmExit(code: number) {
				this.exitCode = code;
			},
		};
		const getProcessSnapshot = vi
			.fn()
			.mockRejectedValueOnce(blocked)
			.mockResolvedValue([
				{
					processId: "owned",
					status: "exited",
					exitCode: 23,
					exitSignal: null,
				},
			]);
		const finishProcess = vi.fn();
		const handleBackgroundProcessError = vi.fn((_entry, error) => {
			entry.waitError = error;
		});
		const proxy = Object.assign(Object.create(SidecarKernelProxy.prototype), {
			disposed: false,
			disposing: false,
			client: { getProcessSnapshot },
			session: {},
			vm: {},
			processSnapshotRefresh: null,
			sidecarProcessSnapshot: [],
			finishProcess,
			handleBackgroundProcessError,
		});
		const exit = proxy.waitForTrackedProcess(entry);
		// Attach before advancing timers: the red case is a rejected observation, not an unhandled rejection.
		const outcome = exit.then(
			(code: number) => ({ code }),
			(error: unknown) => ({ error }),
		);
		await vi.advanceTimersByTimeAsync(500);
		expect(getProcessSnapshot).not.toHaveBeenCalled();
		expect(entry.waitError).toBeNull();
		entry.waitError = blocked;
		entry.wakeWait?.();
		expect(await outcome).toEqual({ error: blocked });
		expect(getProcessSnapshot).not.toHaveBeenCalled();
		expect(finishProcess).not.toHaveBeenCalled();
	} finally {
		vi.useRealTimers();
	}
});

it("process wait coalesces until the ordered exit event without snapshot polling", async () => {
	const entry = {
		exitCode: null as number | null,
		waitError: null,
		waitPromise: null,
		wakeWait: null as (() => void) | null,
	};
	const getProcessSnapshot = vi.fn();
	const proxy = Object.assign(Object.create(SidecarKernelProxy.prototype), {
		disposed: false,
		disposing: false,
		client: { getProcessSnapshot },
	});
	const first = proxy.waitForTrackedProcess(entry);
	expect(proxy.waitForTrackedProcess(entry)).toBe(first);
	let settled = false;
	void first.then(() => {
		settled = true;
	});
	await Promise.resolve();
	expect(settled).toBe(false);
	expect(getProcessSnapshot).not.toHaveBeenCalled();
	entry.exitCode = 23;
	entry.wakeWait?.();
	expect(await first).toBe(23);
});

it.each([
	true,
	false,
])("process wait lets bounded disposal finish before deciding exit proof (%s)", async (confirmed) => {
	vi.useFakeTimers();
	try {
		const entry = {
			pid: 1,
			exitCode: null as number | null,
			waitError: null as Error | null,
			waitPromise: null,
			wakeWait: null as (() => void) | null,
		};
		const proxy = Object.assign(Object.create(SidecarKernelProxy.prototype), {
			disposed: false,
			disposing: false,
		});
		const wait = proxy.waitForTrackedProcess(entry);
		let settled = false;
		const result = wait.then(
			(code: number) => {
				settled = true;
				return { code };
			},
			(error: Error) => {
				settled = true;
				return { error };
			},
		);
		proxy.disposed = true;
		proxy.disposing = true;
		entry.wakeWait?.();
		await vi.advanceTimersByTimeAsync(2000);
		expect(settled).toBe(false);
		proxy.disposing = false;
		if (confirmed) entry.exitCode = 143;
		else entry.waitError = new Error("termination unconfirmed");
		entry.wakeWait?.();
		expect(await result).toEqual(
			confirmed ? { code: 143 } : { error: entry.waitError },
		);
	} finally {
		vi.useRealTimers();
	}
});
