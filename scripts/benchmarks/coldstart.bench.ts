/**
 * Cold-start latency benchmark.
 *
 * Measures time from AgentOs.create() through workload ready:
 *   --workload=echo             Minimal VM + first exec("echo hello") completing
 *   --workload=pi-session       VM + createSession("pi") completing (ACP handshake done)
 *   --workload=pi-rust-session  VM + createSession("pi-rust") completing (ACP handshake done)
 *   --workload=claude-session   VM + createSession("claude") completing (ACP handshake done)
 *   --workload=codex-session    VM + createSession("codex") completing (ACP handshake done)
 *
 * Pass --iterations=N to override default (5).
 *
 * Usage:
 *   npx tsx scripts/benchmarks/coldstart.bench.ts --workload=echo
 *   npx tsx scripts/benchmarks/coldstart.bench.ts --workload=pi-session --iterations=3
 *   npx tsx scripts/benchmarks/coldstart.bench.ts --workload=pi-rust-session --iterations=3
 *   npx tsx scripts/benchmarks/coldstart.bench.ts --workload=claude-session --iterations=3
 */

import {
	ITERATIONS,
	WARMUP_ITERATIONS,
	WORKLOADS,
	createBenchVm,
	ECHO_COMMAND,
	EXPECTED_OUTPUT,
	getHardware,
	printTable,
	round,
	stats,
	stopLlmock,
} from "./bench-utils.js";

const VALID_WORKLOADS = ["echo", ...Object.keys(WORKLOADS).filter((k) => k.endsWith("-session"))];

async function measureEcho(): Promise<number> {
	const t0 = performance.now();
	const vm = await createBenchVm();
	const result = await vm.exec(ECHO_COMMAND);
	const ms = performance.now() - t0;
	if (result.stdout !== EXPECTED_OUTPUT) {
		throw new Error(`Unexpected output: ${JSON.stringify(result.stdout)}`);
	}
	await vm.dispose();
	return ms;
}

async function measureAgentSession(workloadName: string): Promise<number> {
	const workload = WORKLOADS[workloadName];
	const t0 = performance.now();
	const vm = await workload.createVm();
	await workload.start(vm);
	const ms = performance.now() - t0;
	await vm.dispose();
	return ms;
}

function parseArgs(): { workload: string; iterations: number } {
	const wArg = process.argv.find((a) => a.startsWith("--workload="));
	const iArg = process.argv.find((a) => a.startsWith("--iterations="));

	if (!wArg) {
		console.error(
			`Usage: npx tsx coldstart.bench.ts --workload=${VALID_WORKLOADS.join("|")} [--iterations=N]`,
		);
		process.exit(1);
	}
	const name = wArg.split("=")[1];
	if (!VALID_WORKLOADS.includes(name)) {
		console.error(`Unknown workload: ${name}. Use: ${VALID_WORKLOADS.join(", ")}`);
		process.exit(1);
	}

	let iterations = ITERATIONS;
	if (iArg) {
		const val = parseInt(iArg.split("=")[1], 10);
		if (!isNaN(val) && val >= 1) iterations = val;
	}

	return { workload: name, iterations };
}

async function main() {
	const { workload, iterations } = parseArgs();
	const measure = workload === "echo"
		? measureEcho
		: () => measureAgentSession(workload);

	const hardware = getHardware();
	console.error(`=== Cold-Start Benchmark (${workload}) ===`);
	console.error(`CPU: ${hardware.cpu}`);
	console.error(`RAM: ${hardware.ram} | Node: ${hardware.node}`);
	console.error(`Iterations: ${iterations} (+ ${WARMUP_ITERATIONS} warmup)`);

	const samples: number[] = [];

	for (let i = 0; i < WARMUP_ITERATIONS + iterations; i++) {
		const ms = await measure();
		if (i >= WARMUP_ITERATIONS) {
			samples.push(ms);
		}
		console.error(
			`  iter ${i}: ${round(ms)}ms${i < WARMUP_ITERATIONS ? " (warmup)" : ""}`,
		);
	}

	const s = stats(samples);

	printTable(
		["metric", "mean", "p50", "p95", "min", "max"],
		[["cold start", `${s.mean}ms`, `${s.p50}ms`, `${s.p95}ms`, `${s.min}ms`, `${s.max}ms`]],
	);

	console.log(
		JSON.stringify({ hardware, workload, iterations, coldStart: s }, null, 2),
	);

	await stopLlmock();
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
