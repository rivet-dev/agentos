import { describe, expect, it } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import type { ManagedProcess } from "../src/runtime-compat.js";
import { SidecarKernelProxy } from "../src/sidecar/rpc-client.js";

describe("process topology after output routing retirement", () => {
	it.each([
		{ code: 143, signal: 15 },
		{ code: 143, signal: null },
		{ code: 0, signal: null },
	])("preserves tracked identity and ($code,$signal) with present/reaped native rows", async ({
		code,
		signal,
	}) => {
		const tracked = {
			processId: "owned-root",
			pid: 1_000_000,
			driver: "node",
			command: "tracked",
			args: ["-e", "source"],
			cwd: "/",
			startTime: 1,
			exitTime: 2,
			exitCode: code,
			exitSignal: signal,
			onStdout: new Set(),
			onStderr: new Set(),
		};
		const native = {
			processId: tracked.processId,
			pid: 42,
			ppid: 0,
			pgid: 42,
			sid: 42,
			driver: "node",
			command: "native-command",
			args: ["-e"],
			cwd: "/",
			status: "exited" as const,
			exitCode: code,
			exitSignal: signal,
		};
		const child = {
			...native,
			processId: "guest-child",
			pid: 1_000_000,
			ppid: 42,
			command: "guest",
			status: "running" as const,
			exitCode: null,
			exitSignal: null,
		};
		let rows: Array<typeof native | typeof child> = [native, child];
		const proxy = Object.create(
			SidecarKernelProxy.prototype,
		) as SidecarKernelProxy;
		Object.assign(proxy, {
			client: { getProcessSnapshot: async () => rows },
			session: {},
			vm: {},
			processSnapshotRefresh: null,
			sidecarProcessSnapshot: [],
			trackedProcesses: new Map([[tracked.pid, tracked]]),
			trackedProcessesById: new Map([[tracked.processId, tracked]]),
			observedProcessStartTimes: new Map(),
			processes: new Map(),
			signalRefreshes: new Map(),
			signalStates: new Map(),
			localMounts: [],
			drainTrailingProcessOutput: async () => {},
		});
		const vm = Reflect.construct(AgentOs, [
			proxy,
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
		const process: ManagedProcess = {
			pid: tracked.pid,
			processId: tracked.processId,
			exitCode: code,
			exitSignal: signal,
			wait: async () => code,
			kill: () => {},
			writeStdin: async () => {},
			closeStdin: async () => {},
		};
		const internal = vm as unknown as {
			_trackProcess: (...args: unknown[]) => void;
			_processes: Map<number, unknown>;
		};
		internal._trackProcess(
			process,
			tracked.command,
			tracked.args,
			true,
			new Set(),
			new Set(),
		);
		const retirement = Reflect.get(
			proxy,
			"releaseProcessTrackingAfterDrain",
		) as (entry: unknown) => Promise<void>;
		await retirement.call(proxy, tracked);
		expect(proxy.__trackingSizesForTest()).toMatchObject({
			trackedProcesses: 0,
			trackedProcessesById: 0,
			signalStates: 0,
			signalRefreshes: 0,
		});
		expect(tracked.onStdout.size).toBe(0);
		expect(tracked.onStderr.size).toBe(0);

		const present = await vm.process.tree();
		const root = present.find((node) => node.command === "tracked");
		expect(root).toMatchObject({
			pid: tracked.pid,
			state: "exited",
			exitCode: code,
		});
		expect(root?.exitSignal ?? null).toBe(signal);
		expect(root?.children).toHaveLength(1);
		expect(root?.children[0]).toMatchObject({
			pid: tracked.pid,
			ppid: tracked.pid,
			command: "guest",
			state: "running",
		});

		// The sidecar reaps the parent, but Core still owns its bounded process record.
		rows = [child];
		const reaped = await vm.process.tree();
		expect(reaped).toHaveLength(2);
		const retained = reaped.find((node) => node.command === "tracked");
		expect(retained).toMatchObject({
			pid: tracked.pid,
			state: "exited",
			exitCode: code,
			children: [],
		});
		expect(retained?.exitSignal ?? null).toBe(signal);
		expect(reaped.find((node) => node.command === "guest")).toMatchObject({
			pid: tracked.pid,
			state: "running",
		});
		expect(proxy.processes.get(tracked.pid)?.command).toBe("tracked");

		// No second retained registry survives Core eviction/disposal.
		internal._processes.clear();
		rows = [];
		expect(await vm.process.tree()).toEqual([]);
		expect(proxy.__trackingSizesForTest()).toMatchObject({
			trackedProcesses: 0,
			trackedProcessesById: 0,
		});
	});
});
