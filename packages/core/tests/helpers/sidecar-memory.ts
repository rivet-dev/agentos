import { readFileSync } from "node:fs";
import type { AgentOs } from "../../src/index.js";
import type { StdioSidecarProcess } from "../../src/process.js";

// Reaches into internals only to find the sidecar process; the behavior under
// test is the memory the OS reports for it.
export function sidecarPid(vm: AgentOs): number {
	const child = (
		vm as unknown as {
			_sidecarClient: {
				protocolClient: { sidecarProcess: StdioSidecarProcess };
			};
		}
	)._sidecarClient.protocolClient.sidecarProcess.child;
	if (child.pid === undefined) throw new Error("sidecar process has no pid");
	return child.pid;
}

/** Proportional set size the OS reports for a process (Linux only). */
export function pssBytes(pid: number): number {
	const rollup = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8");
	const match = rollup.match(/^Pss:\s+(\d+) kB$/m);
	if (!match) throw new Error(`no Pss line in /proc/${pid}/smaps_rollup`);
	return Number(match[1]) * 1024;
}
