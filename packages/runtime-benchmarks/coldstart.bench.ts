/**
 * Cold-start, sidecar-reuse, and resident-runner latency benchmark for the
 * Secure Exec runtime.
 *
 * Scenarios:
 *   - owned-sidecar: `NodeRuntime.create()` owns a fresh sidecar per runtime.
 *   - shared-sidecar: one `Sidecar` is created outside the measurement and
 *     passed to `NodeRuntime.create({ sidecar })`.
 *   - resident-runner: one shared sidecar plus a live guest Node process reused
 *     through `runtime.createResidentRunner()`.
 */

import {
	BATCH_SIZES,
	type BenchScenario,
	createBenchRuntime,
	createBenchSidecar,
	createBootTimingRecorder,
	EXEC_TIMEOUT_MS,
	getHardware,
	ITERATIONS,
	MAX_CONCURRENCY,
	MAX_LIVE_RUNTIMES,
	MAX_RESIDENT_RUNNERS,
	mergePhaseSamples,
	type PhaseSamples,
	printTable,
	RESIDENT_TRIVIAL_CODE,
	SCENARIOS,
	stats,
	summarizePhases,
	TRIVIAL_CODE,
	WARMUP_ITERATIONS,
} from "./bench-utils.js";

type BenchMode = "sequential" | "concurrent";

interface Measurement {
	coldMs: number;
	warmMs: number;
	phases: PhaseSamples;
}

interface ColdStartEntry {
	scenario: BenchScenario;
	batchSize: number;
	mode: BenchMode;
	iterations: number;
	sidecarSetupMs?: ReturnType<typeof stats>;
	coldStart: ReturnType<typeof stats>;
	warmStart: ReturnType<typeof stats>;
	phases: ReturnType<typeof summarizePhases>;
}

async function measureRuntime(sidecar?: ReturnType<typeof createBenchSidecar>) {
	const phases: PhaseSamples = {};
	const t0 = performance.now();
	const runtime = await createBenchRuntime({
		...(sidecar ? { sidecar } : {}),
		onBootTiming: createBootTimingRecorder(phases),
	});
	(phases.runtime_create_total ??= []).push(performance.now() - t0);
	return { runtime, phases };
}

async function measureExecPair(
	sidecar?: ReturnType<typeof createBenchSidecar>,
): Promise<Measurement> {
	const { runtime, phases } = await measureRuntime(sidecar);
	try {
		const firstStart = performance.now();
		await runtime.exec(TRIVIAL_CODE, { timeout: EXEC_TIMEOUT_MS });
		const firstMs = performance.now() - firstStart;
		(phases.first_exec ??= []).push(firstMs);

		const warmStart = performance.now();
		await runtime.exec(TRIVIAL_CODE, { timeout: EXEC_TIMEOUT_MS });
		const warmMs = performance.now() - warmStart;
		(phases.warm_exec ??= []).push(warmMs);

		return {
			coldMs:
				(phases.runtime_create_total?.at(-1) ?? 0) + firstMs,
			warmMs,
			phases,
		};
	} finally {
		await runtime.dispose();
	}
}

async function measureResidentPair(
	sidecar: ReturnType<typeof createBenchSidecar>,
): Promise<Measurement> {
	const { runtime, phases } = await measureRuntime(sidecar);
	try {
		const runnerStart = performance.now();
		const runner = await runtime.createResidentRunner();
		const runnerCreateMs = performance.now() - runnerStart;
		(phases.resident_runner_create ??= []).push(runnerCreateMs);

		try {
			const firstStart = performance.now();
			await runner.exec(RESIDENT_TRIVIAL_CODE, { timeout: EXEC_TIMEOUT_MS });
			const firstMs = performance.now() - firstStart;
			(phases.resident_first_exec ??= []).push(firstMs);

			const warmStart = performance.now();
			await runner.exec(RESIDENT_TRIVIAL_CODE, { timeout: EXEC_TIMEOUT_MS });
			const warmMs = performance.now() - warmStart;
			(phases.resident_warm_exec ??= []).push(warmMs);

			return {
				coldMs:
					(phases.runtime_create_total?.at(-1) ?? 0) +
					runnerCreateMs +
					firstMs,
				warmMs,
				phases,
			};
		} finally {
			await runner.dispose();
		}
	} finally {
		await runtime.dispose();
	}
}

function appendMeasurements(
	target: {
		coldSamples: number[];
		warmSamples: number[];
		phaseSamples: PhaseSamples;
	},
	measurements: Measurement[],
) {
	for (const measurement of measurements) {
		target.coldSamples.push(measurement.coldMs);
		target.warmSamples.push(measurement.warmMs);
		mergePhaseSamples(target.phaseSamples, measurement.phases);
	}
}

async function collectBatch(
	scenario: BenchScenario,
	batchSize: number,
	mode: BenchMode,
): Promise<{
	measurements: Measurement[];
	sidecarSetupMs?: number;
}> {
	if (scenario === "owned-sidecar") {
		const concurrency =
			mode === "sequential" ? 1 : Math.min(batchSize, MAX_LIVE_RUNTIMES);
		return {
			measurements: await runScenarioBatch(batchSize, concurrency, () =>
				measureExecPair(),
			),
		};
	}

	const sidecarStart = performance.now();
	const sidecar = createBenchSidecar();
	const sidecarSetupMs = performance.now() - sidecarStart;
	try {
		const concurrency =
			scenario === "resident-runner"
				? mode === "sequential"
					? 1
					: Math.min(batchSize, MAX_RESIDENT_RUNNERS)
				: mode === "sequential"
					? 1
					: Math.min(batchSize, MAX_LIVE_RUNTIMES);
		const measure =
			scenario === "resident-runner"
				? () => measureResidentPair(sidecar)
				: () => measureExecPair(sidecar);
		return {
			measurements: await runScenarioBatch(batchSize, concurrency, measure),
			sidecarSetupMs,
		};
	} finally {
		await sidecar.dispose();
	}
}

async function runScenarioBatch(
	batchSize: number,
	concurrency: number,
	measure: () => Promise<Measurement>,
) {
	const { runLimited } = await import("./bench-utils.js");
	return runLimited(batchSize, concurrency, measure);
}

async function benchScenario(
	scenario: BenchScenario,
	batchSize: number,
	mode: BenchMode,
): Promise<ColdStartEntry> {
	const coldSamples: number[] = [];
	const warmSamples: number[] = [];
	const sidecarSetupSamples: number[] = [];
	const phaseSamples: PhaseSamples = {};

	for (let iter = 0; iter < WARMUP_ITERATIONS + ITERATIONS; iter++) {
		const batch = await collectBatch(scenario, batchSize, mode);
		if (iter >= WARMUP_ITERATIONS) {
			appendMeasurements(
				{ coldSamples, warmSamples, phaseSamples },
				batch.measurements,
			);
			if (batch.sidecarSetupMs !== undefined) {
				sidecarSetupSamples.push(batch.sidecarSetupMs);
			}
		}
	}

	return {
		scenario,
		batchSize,
		mode,
		iterations: ITERATIONS,
		...(sidecarSetupSamples.length > 0
			? { sidecarSetupMs: stats(sidecarSetupSamples) }
			: {}),
		coldStart: stats(coldSamples),
		warmStart: stats(warmSamples),
		phases: summarizePhases(phaseSamples),
	};
}

function formatPhaseSummary(phases: ColdStartEntry["phases"]): string {
	return Object.entries(phases)
		.map(([phase, phaseStats]) => `${phase}.p50=${phaseStats.p50}ms`)
		.join(" | ");
}

async function main() {
	const hardware = getHardware();
	console.error("=== Cold Start Benchmark ===");
	console.error(`CPU: ${hardware.cpu}`);
	console.error(
		`Cores: ${hardware.cores} | Max concurrency: ${MAX_CONCURRENCY} | Max live runtimes: ${MAX_LIVE_RUNTIMES}`,
	);
	console.error(`Resident runner live cap: ${MAX_RESIDENT_RUNNERS}`);
	console.error(`RAM: ${hardware.ram} | Node: ${hardware.node}`);
	console.error(`Loadavg: ${hardware.loadAverage.join(", ")}`);
	console.error(
		`MemAvailable: ${hardware.memAvailable ?? "unknown"} | SwapFree: ${hardware.swapFree ?? "unknown"} | SwapCached: ${hardware.swapCached ?? "unknown"}`,
	);
	console.error(`Iterations: ${ITERATIONS} (+ ${WARMUP_ITERATIONS} warmup)`);
	console.error(`Batch sizes: ${BATCH_SIZES.join(", ")}`);
	console.error(`Scenarios: ${SCENARIOS.join(", ")}`);
	console.error(
		`Sidecar: ${process.env.AGENTOS_SIDECAR_BIN ?? process.env.AGENTOS_SIDECAR_BIN ?? "(resolved from @rivet-dev/agentos-runtime-sidecar)"}\n`,
	);

	const results: ColdStartEntry[] = [];

	for (const scenario of SCENARIOS) {
		for (const batchSize of BATCH_SIZES) {
			for (const mode of ["sequential", "concurrent"] as const) {
				console.error(
					`\n--- scenario=${scenario}, batch=${batchSize}, mode=${mode} ---`,
				);
				const entry = await benchScenario(scenario, batchSize, mode);
				results.push(entry);
				console.error(
					`  cold: mean=${entry.coldStart.mean}ms p50=${entry.coldStart.p50}ms p95=${entry.coldStart.p95}ms`,
				);
				console.error(
					`  warm: mean=${entry.warmStart.mean}ms p50=${entry.warmStart.p50}ms p95=${entry.warmStart.p95}ms`,
				);
				console.error(`  phases: ${formatPhaseSummary(entry.phases)}`);
				if (entry.sidecarSetupMs) {
					console.error(
						`  sidecar setup excluded: p50=${entry.sidecarSetupMs.p50}ms p95=${entry.sidecarSetupMs.p95}ms`,
					);
				}
			}
		}
	}

	printTable(
		[
			"scenario",
			"batch",
			"mode",
			"cold mean",
			"cold p50",
			"cold p95",
			"warm mean",
			"warm p50",
			"warm p95",
		],
		results.map((r) => [
			r.scenario,
			r.batchSize,
			r.mode,
			`${r.coldStart.mean}ms`,
			`${r.coldStart.p50}ms`,
			`${r.coldStart.p95}ms`,
			`${r.warmStart.mean}ms`,
			`${r.warmStart.p50}ms`,
			`${r.warmStart.p95}ms`,
		]),
	);

	console.log(JSON.stringify({ hardware, scenarioSetup: {}, results }, null, 2));
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
