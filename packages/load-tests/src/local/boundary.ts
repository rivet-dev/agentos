// Container-boundary self-test (docs-internal/load-testing.md checklist C).
//
// Runs inside the SAME bounded container recipe as the adversarial lanes and
// verifies that the cgroup envelope, fd ulimit, tmpfs, and network isolation
// actually match what the `just` recipe requested. This is how we prove the
// sandbox is real before trusting any "the sidecar survived" verdict.
import { readFileSync, statfsSync } from "node:fs";
import { createConnection } from "node:net";
import {
	cgroupSnapshot,
	errorText,
	newRunId,
	numberEnv,
	runtimeProvenance,
	writeArtifact,
} from "../common.js";

function readSelfLimit(name: string): { soft: string; hard: string } | undefined {
	try {
		for (const line of readFileSync("/proc/self/limits", "utf8").split("\n")) {
			// Columns are space-padded: "Max open files  1024  1024  files".
			if (line.startsWith(name)) {
				const cols = line.slice(name.length).trim().split(/\s+/);
				return { soft: cols[0]!, hard: cols[1]! };
			}
		}
	} catch {
		// Non-Linux / unreadable.
	}
	return undefined;
}

async function networkReachable(timeoutMs: number): Promise<boolean> {
	// Local lanes run with --network=none, so an outbound TCP connect must fail.
	return new Promise((resolve) => {
		const socket = createConnection({ host: "1.1.1.1", port: 443 });
		const done = (reachable: boolean) => {
			socket.destroy();
			resolve(reachable);
		};
		socket.setTimeout(timeoutMs);
		socket.once("connect", () => done(true));
		socket.once("timeout", () => done(false));
		socket.once("error", () => done(false));
	});
}

export async function runBoundary(): Promise<void> {
	const runId = newRunId("boundary");
	const cgroup = cgroupSnapshot();
	const nofile = readSelfLimit("Max open files");
	const procs = readSelfLimit("Max processes");

	let tmp: { totalBytes: number } | undefined;
	try {
		const s = statfsSync("/tmp");
		tmp = { totalBytes: Number(s.blocks) * Number(s.bsize) };
	} catch (error) {
		tmp = undefined;
		console.error(`statfs(/tmp) failed: ${errorText(error)}`);
	}

	const networkExpectedReachable = process.env.LOAD_TEST_EXPECT_NETWORK === "1";
	const reachable = await networkReachable(numberEnv("LOAD_TEST_NET_PROBE_MS", 2_000));

	const memMax = cgroup["memory.max"];
	const swapMax = cgroup["memory.swap.max"];
	const pidsMax = cgroup["pids.max"];

	const failures: string[] = [];
	if (typeof memMax !== "number") failures.push("memory.max is not a numeric cap (unbounded?)");
	// Equal memory+swap (i.e. no extra swap) is enforced by --memory-swap == --memory,
	// which makes memory.swap.max == 0 under cgroup v2.
	if (swapMax !== 0 && swapMax !== "0") {
		failures.push(`memory.swap.max is ${String(swapMax)}; expected 0 (no extra swap)`);
	}
	if (typeof pidsMax !== "number") failures.push("pids.max is not a numeric cap (unbounded?)");
	if (!nofile) failures.push("could not read RLIMIT_NOFILE");
	if (!tmp) failures.push("could not statfs /tmp");
	if (reachable !== networkExpectedReachable) {
		failures.push(
			`network reachable=${reachable} but expected reachable=${networkExpectedReachable}`,
		);
	}

	const artifact = {
		runId,
		lane: "container-boundary",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		envelope: {
			memoryMax: memMax,
			memorySwapMax: swapMax,
			pidsMax,
			nofile,
			maxProcesses: procs,
			tmpTotalBytes: tmp?.totalBytes,
			networkReachable: reachable,
			networkExpectedReachable,
		},
		cgroup,
		provenance: runtimeProvenance(),
	};
	const path = writeArtifact("boundary", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures, path, envelope: artifact.envelope }));
	if (failures.length > 0) process.exitCode = 1;
}
