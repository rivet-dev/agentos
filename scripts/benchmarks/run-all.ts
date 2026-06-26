import { AgentOs } from "@rivet-dev/agentos-core";
import { allOps } from "./families/index.js";
import { runOp, type OpResult } from "./lib/layers.js";
import { findingsFromLatency, refutedFromLatency, writeJson } from "./lib/report.js";
import { getHardware, printTable } from "./lib/perf-utils.js";
import { runFuzz } from "./fuzz/run.js";
import { runLeakSuite } from "./leak.js";
import { runFootprint } from "./footprint.js";
import { compareBaselineFile } from "./compare-baseline.js";

const RESULTS_DIR = new URL("./results/", import.meta.url).pathname;
const ITERATIONS = Number(process.env.BENCH_ITERATIONS ?? 20);
const WARMUP = Number(process.env.BENCH_WARMUP ?? 5);

export async function runLatencyMatrix(): Promise<OpResult[]> {
	const vm = await AgentOs.create({
		permissions: { fs: "allow", network: "allow", childProcess: "allow", process: "allow" },
	});
	try {
		const results: OpResult[] = [];
		for (const op of allOps) {
			console.error(`latency ${op.family}/${op.name}`);
			results.push(await runOp(op, vm, ITERATIONS, WARMUP));
		}
		return results;
	} finally {
		await vm.dispose();
	}
}

async function main(): Promise<void> {
	const latency = await runLatencyMatrix();
	const findings = findingsFromLatency(latency);
	const refuted = refutedFromLatency(latency);
	const fuzz = await runFuzz({ iterations: ITERATIONS, warmup: WARMUP });
	const leak = await runLeakSuite();
	const footprint = await runFootprint();
	const findingsJson = {
		generatedAt: new Date().toISOString(),
		hardware: getHardware(),
		iterations: ITERATIONS,
		warmup: WARMUP,
		latency,
		fuzz,
		leak,
		footprint,
		findings: [
			...findings,
			...fuzz.findings,
			...leak.findings,
			...footprint.findings,
		].sort((a, b) => b.emulation_ratio - a.emulation_ratio),
		refuted: [
			...refuted,
			...fuzz.refuted,
			{
				family: "net",
				op: "udp_echo",
				reason: "guest UDP datagrams are unsupported in the current kernel-backed V8 bridge",
				evidence: "ERR_NOT_IMPLEMENTED: external UDP datagrams are not yet supported by the kernel-backed V8 bridge",
			},
		],
		critic_gaps: criticGaps(latency, fuzz, leak, footprint),
	};
	writeJson(`${RESULTS_DIR}/latency-matrix.json`, { latency });
	writeJson(`${RESULTS_DIR}/findings.json`, findingsJson);
	const baselinePath = `${RESULTS_DIR}/baseline/findings-baseline.json`;
	const diff = compareBaselineFile(`${RESULTS_DIR}/findings.json`, baselinePath);
	writeJson(`${RESULTS_DIR}/regression-diff.json`, diff);

	printTable(
		["family", "op", "guest/node", "guest/native", "file:line"],
		findingsJson.findings.map((finding) => [
			finding.family,
			finding.op,
			finding.emulation_ratio,
			finding.total_ratio,
			finding.file_line,
		]),
	);
	console.log(JSON.stringify(findingsJson, null, 2));
}

function criticGaps(
	latency: OpResult[],
	fuzz: Awaited<ReturnType<typeof runFuzz>>,
	leak: Awaited<ReturnType<typeof runLeakSuite>>,
	footprint: Awaited<ReturnType<typeof runFootprint>>,
): string[] {
	const gaps: string[] = [];
	const covered = new Set(latency.map((result) => `${result.family}/${result.op}`));
	for (const required of [
		"process/fanout_spawn_8",
		"process/wait_reap_storm_8",
		"fs/readdir_large",
		"dns/resolve_concurrent_4",
		"pipes/backpressure_chunks",
		"control/cpu_loop",
	]) {
		if (!covered.has(required)) gaps.push(`missing fixed op ${required}`);
	}
	gaps.push(
		"unsupported fixed op net/udp_echo: guest dgram send returns ERR_NOT_IMPLEMENTED for external UDP datagrams",
	);
	if (!fuzz.findings.some((finding) => finding.op === "fanout-stdout-storm")) {
		gaps.push("fuzz did not confirm the non-P2 stdout fanout slow path");
	}
	if (leak.streams.some((stream) => stream.idleMs < 61_000)) {
		gaps.push("leak suite was run in smoke mode without waiting past 60s ZOMBIE_TTL");
	}
	if (footprint.components?.length === 0) {
		gaps.push("footprint run did not emit component attribution");
	}
	return gaps;
}

main().then(
	() => process.exit(0),
	(error) => {
		console.error(error);
		process.exit(1);
	},
);
