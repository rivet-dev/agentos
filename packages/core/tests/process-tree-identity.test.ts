import { describe, expect, it, vi } from "vitest";
import {
	buildProcessForest,
	type ProcessForestRecord,
} from "../src/process-forest.js";
import { SidecarKernelProxy } from "../src/sidecar/rpc-client.js";

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
			getProcessSnapshot: async () => [],
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
			sidecarProcessSnapshot: [
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
			trackedProcesses: new Map([[tracked.pid, tracked]]),
			trackedProcessesById: new Map([[tracked.processId, tracked]]),
			observedProcessStartTimes: new Map(),
			processes: new Map(),
		});
		const records = proxy.snapshotProcessTopology();
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
		await Promise.resolve();
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
