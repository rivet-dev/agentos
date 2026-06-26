import { readdirSync, readFileSync } from "node:fs";
import { AgentOs } from "@rivet-dev/agentos-core";
import { forceGC } from "./perf-utils.js";

export interface MemorySample {
	cycle: number;
	guestHeapRss: number;
	sidecarRss: number;
	runningProcesses: number;
	exitedProcesses: number;
	openFds: number;
	sockets: number;
	pipes: number;
}

export function findSidecarPid(): number | null {
	return findSidecarPids()[0] ?? null;
}

export function findSidecarPids(): number[] {
	const pids: number[] = [];
	for (const pid of readdirSync("/proc")) {
		if (!/^\d+$/.test(pid)) continue;
		try {
			const comm = readFileSync(`/proc/${pid}/comm`, "utf8").trim();
			if (comm === "agentos-sidecar" || comm === "secure-exec-sidecar") {
				pids.push(Number(pid));
			}
		} catch {
			// Process exited while scanning.
		}
	}
	return pids.sort((a, b) => a - b);
}

export function readRssBytes(pid: number | null): number {
	if (pid === null) return 0;
	try {
		const status = readFileSync(`/proc/${pid}/status`, "utf8");
		const match = status.match(/^VmRSS:\s+(\d+)\s+kB/m);
		return match ? Number(match[1]) * 1024 : 0;
	} catch {
		return 0;
	}
}

export async function sampleMemory(vm: AgentOs, cycle: number): Promise<MemorySample> {
	forceGC();
	const resource = await vm.getResourceSnapshot();
	const guestHeapRss = await sampleGuestHeap(vm);
	return {
		cycle,
		guestHeapRss,
		sidecarRss: readRssBytes(findSidecarPid()),
		runningProcesses: resource.runningProcesses,
		exitedProcesses: resource.exitedProcesses,
		openFds: resource.openFds,
		sockets: resource.sockets,
		pipes: resource.pipes,
	};
}

export function slope(samples: Array<{ cycle: number }>, key: string): number {
	const n = samples.length;
	const sx = samples.reduce((sum, sample) => sum + sample.cycle, 0);
	const sy = samples.reduce((sum, sample) => sum + Number((sample as any)[key]), 0);
	const sxy = samples.reduce(
		(sum, sample) => sum + sample.cycle * Number((sample as any)[key]),
		0,
	);
	const sx2 = samples.reduce((sum, sample) => sum + sample.cycle ** 2, 0);
	const denom = n * sx2 - sx ** 2;
	return denom === 0 ? 0 : (n * sxy - sx * sy) / denom;
}

async function sampleGuestHeap(vm: AgentOs): Promise<number> {
	const script = "/tmp/guest-memory-usage.mjs";
	await vm.writeFile(
		script,
		"process.stdout.write(String(process.memoryUsage().rss));",
	);
	let stdout = "";
	const proc = vm.spawn("node", [script], {
		onStdout: (data) => {
			stdout += Buffer.from(data).toString("utf8");
		},
	});
	const code = await vm.waitProcess(proc.pid);
	if (code !== 0) return 0;
	return Number(stdout.trim() || 0);
}
