import { describe, expect, it } from "vitest";
import { processRowIdentity } from "../../agentos/src/inspector-tabs/lib/process-identity";
import {
	buildProcessForest,
	type ProcessForestRecord,
} from "../src/process-forest.js";
import { SidecarKernelProxy } from "../src/sidecar/rpc-client.js";

describe("process-tree identity", () => {
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

	it("gives colliding inspector rows distinct identities", () => {
		const tracked = {
			generation: 4n,
			pid: 1_000_000,
			process: { generation: 4n, pid: 1_000_000 },
		};
		const guest = { ...tracked, process: null };
		expect(processRowIdentity(tracked)).not.toBe(processRowIdentity(guest));
		expect(processRowIdentity(guest)).not.toBe(
			processRowIdentity({ ...guest, generation: 5n }),
		);
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
});
