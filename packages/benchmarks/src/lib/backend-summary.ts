import type { ProcessMemorySnapshot } from "./memory.js";
import { retainedMemoryMedian } from "./backend-memory.js";

type Backend = "v8" | "wasmtime";
const expectedWorkloads = [
	"trivial",
	"coreutils",
	"shell",
	"curl",
	"sqlite",
	"vim",
	"large-module",
	"compute-heavy",
	"host-call-heavy",
];

export function summarizeBackendComparison(result: Record<string, unknown>) {
	const fresh = (result.fresh ?? []) as Array<{
		backend: Backend;
		processIndex: number;
		retained: ProcessMemorySnapshot;
		retainedDelta: ProcessMemorySnapshot;
		workloads: Array<{
			name: string;
			samples: Array<{
				durationMs: number;
				cacheState: "fresh" | "warm";
				passed: boolean;
			}>;
		}>;
	}>;
	const workloadNames = expectedWorkloads;
	const workloadRows = workloadNames.map((name) => {
		const samples = (backend: Backend, cacheState?: "fresh" | "warm") =>
			fresh
				.filter((entry) => entry.backend === backend)
				.flatMap(
					(entry) =>
						entry.workloads.find((candidate) => candidate.name === name)
							?.samples ?? [],
				)
				.filter(
					(sample) =>
						cacheState === undefined || sample.cacheState === cacheState,
				)
				.map((sample) => sample.durationMs);
		const v8 = samples("v8");
		const wasmtime = samples("wasmtime");
		const v8Cold = samples("v8", "fresh");
		const wasmtimeCold = samples("wasmtime", "fresh");
		const v8Warm = samples("v8", "warm");
		const wasmtimeWarm = samples("wasmtime", "warm");
		return {
			name: name,
			correctness: {
				v8Failures: fresh
					.filter((entry) => entry.backend === "v8")
					.flatMap(
						(entry) =>
							entry.workloads.find((candidate) => candidate.name === name)
								?.samples ?? [],
					)
					.filter((sample) => !sample.passed).length,
				wasmtimeFailures: fresh
					.filter((entry) => entry.backend === "wasmtime")
					.flatMap(
						(entry) =>
							entry.workloads.find((candidate) => candidate.name === name)
								?.samples ?? [],
					)
					.filter((sample) => !sample.passed).length,
			},
			v8: stats(v8),
			wasmtime: stats(wasmtime),
			cold: {
				v8: stats(v8Cold),
				wasmtime: stats(wasmtimeCold),
				p50Ratio: ratio(quantile(wasmtimeCold, 0.5), quantile(v8Cold, 0.5)),
			},
			warm: {
				v8: stats(v8Warm),
				wasmtime: stats(wasmtimeWarm),
				p50Ratio: ratio(quantile(wasmtimeWarm, 0.5), quantile(v8Warm, 0.5)),
			},
			p50Ratio: quantile(wasmtime, 0.5) / quantile(v8, 0.5),
			p95Ratio: quantile(wasmtime, 0.95) / quantile(v8, 0.95),
		};
	});
	const geometricMeanP50Ratio = Math.exp(
		workloadRows.reduce((sum, row) => sum + Math.log(row.p50Ratio), 0) /
			workloadRows.length,
	);
	const concurrency = (result.concurrency ?? []) as Array<{
		backend: Backend;
		levels: Array<{
			level: number;
			mode: string;
			throughputPerSecond: number;
			successful: number;
			failureExamples?: string[];
			rejectionExamples?: string[];
			failedExitCodes: number;
			rejectedCount: number;
		}>;
	}>;
	const admissions = (row: (typeof concurrency)[number]["levels"][number]) => ({
		requested: row.level,
		successful: row.successful,
		failedExitCodes: row.failedExitCodes,
		rejectedCount: row.rejectedCount,
		failureExamples: row.failureExamples ?? [],
		rejectionExamples: row.rejectionExamples ?? [],
	});
	const throughputRows =
		concurrency
			.find((entry) => entry.backend === "v8")
			?.levels.map((v8) => {
				const wasmtime = concurrency
					.find((entry) => entry.backend === "wasmtime")
					?.levels.find(
						(candidate) =>
							candidate.level === v8.level && candidate.mode === v8.mode,
					);
				return {
					admissions: {
						v8: admissions(v8),
						wasmtime: wasmtime ? admissions(wasmtime) : null,
					},
					admissionParity:
						wasmtime !== undefined && wasmtime.successful >= v8.successful,
					level: v8.level,
					mode: v8.mode,
					v8: v8.throughputPerSecond,
					wasmtime: wasmtime?.throughputPerSecond ?? 0,
					ratio: (wasmtime?.throughputPerSecond ?? 0) / v8.throughputPerSecond,
				};
			}) ?? [];
	const retainedDeltaMedian = (
		backend: Backend,
		key: "rssBytes" | "pssBytes",
	) =>
		quantile(
			fresh
				.filter((entry) => entry.backend === backend)
				.map((entry) => entry.retainedDelta[key]),
			0.5,
		);
	const retainedMedian = (backend: Backend, key: "rssBytes" | "pssBytes") =>
		fresh.some((run) => run.backend === backend)
			? retainedMemoryMedian(fresh, backend, key)
			: 0;
	const retainedGrowthAgainstEmptyVm = {
		v8RssBytes: retainedDeltaMedian("v8", "rssBytes"),
		wasmtimeRssBytes: retainedDeltaMedian("wasmtime", "rssBytes"),
		v8PssBytes: retainedDeltaMedian("v8", "pssBytes"),
		wasmtimePssBytes: retainedDeltaMedian("wasmtime", "pssBytes"),
	};
	const retained = {
		v8RssBytes: retainedMedian("v8", "rssBytes"),
		wasmtimeRssBytes: retainedMedian("wasmtime", "rssBytes"),
		v8PssBytes: retainedMedian("v8", "pssBytes"),
		wasmtimePssBytes: retainedMedian("wasmtime", "pssBytes"),
	};
	const retainedAllowance = (baseline: number) =>
		Math.max(baseline * 0.1, 4 * 1024 * 1024);
	const paths = (result.paths ?? []) as Array<{
		backend: Backend;
		denial: { passed: boolean };
		cancellation: { passed: boolean };
		resourceLimit: { passed: boolean };
	}>;
	const metadata = (result.metadata ?? {}) as {
		freshProcesses?: number;
		samplesPerProcess?: number;
		concurrencyLevels?: number[];
	};
	const completenessErrors: string[] = [];
	const positiveInteger = (value: unknown): value is number =>
		typeof value === "number" && Number.isSafeInteger(value) && value > 0;
	if (!positiveInteger(metadata.freshProcesses))
		completenessErrors.push("missing/invalid metadata.freshProcesses");
	if (!positiveInteger(metadata.samplesPerProcess))
		completenessErrors.push("missing/invalid metadata.samplesPerProcess");
	const levels = metadata.concurrencyLevels;
	if (
		!Array.isArray(levels) ||
		!levels.length ||
		!levels.every(positiveInteger) ||
		new Set(levels).size !== levels.length
	) {
		completenessErrors.push("missing/invalid metadata.concurrencyLevels");
	}
	if (fresh.length !== 2 * (metadata.freshProcesses ?? 0))
		completenessErrors.push("unexpected total fresh run count");
	if (paths.length !== 2)
		completenessErrors.push("expected two control-path backends");
	if (concurrency.length !== 2)
		completenessErrors.push("expected two concurrency backends");
	for (const backend of ["v8", "wasmtime"] as const) {
		const runs = fresh.filter((run) => run.backend === backend);
		const indices = new Set(runs.map((run) => run.processIndex));
		if (
			runs.length !== metadata.freshProcesses ||
			indices.size !== runs.length ||
			runs.some(
				(run) =>
					!Number.isInteger(run.processIndex) ||
					run.processIndex < 0 ||
					run.processIndex >= (metadata.freshProcesses ?? 0),
			)
		) {
			completenessErrors.push(`${backend}: incomplete/duplicate fresh runs`);
		}
		for (const run of runs) {
			if (run.workloads.length !== expectedWorkloads.length)
				completenessErrors.push(
					`${backend}/${run.processIndex}: unexpected workload count`,
				);
			for (const name of expectedWorkloads) {
				const matches = run.workloads.filter(
					(workload) => workload.name === name,
				);
				const samples = matches[0]?.samples ?? [];
				if (
					matches.length !== 1 ||
					samples.length !== metadata.samplesPerProcess ||
					samples.filter((sample) => sample.cacheState === "fresh").length !==
						1 ||
					samples.filter((sample) => sample.cacheState === "warm").length !==
						(metadata.samplesPerProcess ?? 0) - 1
				) {
					completenessErrors.push(
						`${backend}/${run.processIndex}/${name}: incomplete samples`,
					);
				}
			}
		}
		if (paths.filter((path) => path.backend === backend).length !== 1)
			completenessErrors.push(`${backend}: missing/duplicate control paths`);
		const comparisons = concurrency.filter((row) => row.backend === backend);
		if (
			comparisons.length !== 1 ||
			comparisons[0]?.levels.length !== 2 * (levels?.length ?? 0)
		)
			completenessErrors.push(`${backend}: incomplete concurrency matrix`);
		for (const level of Array.isArray(levels) ? levels : []) {
			for (const mode of ["repeated", "diverse"]) {
				const matches =
					comparisons[0]?.levels.filter(
						(row) => row.level === level && row.mode === mode,
					) ?? [];
				if (matches.length !== 1)
					completenessErrors.push(
						`${backend}/${level}/${mode}: missing/duplicate concurrency row`,
					);
			}
		}
	}
	const complete = completenessErrors.length === 0;
	const gates = {
		correctness:
			complete &&
			workloadRows.every(
				(row) =>
					row.correctness.v8Failures === 0 &&
					row.correctness.wasmtimeFailures === 0,
			) &&
			paths.every(
				(entry) =>
					entry.denial?.passed === true &&
					entry.cancellation?.passed === true &&
					entry.resourceLimit?.passed === true,
			) &&
			concurrency.every((entry) =>
				entry.levels
					.filter((level) => level.level <= 10)
					.every(
						(level) => level.failedExitCodes === 0 && level.rejectedCount === 0,
					),
			),
		geometricMeanP50: complete && geometricMeanP50Ratio <= 1.1,
		individualP95: complete && workloadRows.every((row) => row.p95Ratio <= 1.2),
		throughput:
			complete &&
			throughputRows.length > 0 &&
			throughputRows.every(
				(row) =>
					row.admissionParity &&
					row.admissions.v8.successful > 0 &&
					(row.admissions.wasmtime?.successful ?? 0) > 0 &&
					(row.v8 === 0 ? row.wasmtime >= row.v8 : row.ratio >= 0.9),
			),
		retainedRss:
			complete &&
			retained.wasmtimeRssBytes <=
				retained.v8RssBytes + retainedAllowance(retained.v8RssBytes),
		retainedPss:
			complete &&
			retained.wasmtimePssBytes <=
				retained.v8PssBytes + retainedAllowance(retained.v8PssBytes),
	};
	const preferredBackend = Object.values(gates).every(Boolean)
		? "wasmtime"
		: "v8";
	return {
		summaryVersion: 3,
		completeness: { passed: complete, errors: completenessErrors },
		workloads: workloadRows,
		geometricMeanP50Ratio,
		throughput: throughputRows,
		retained,
		retainedGrowthAgainstEmptyVm,
		gates,
		preferredBackend,
		omissionBehavior: preferredBackend,
		rollbackBackend: "v8",
	};
}

function stats(values: number[]) {
	if (values.length === 0) {
		return { count: 0, min: null, p50: null, p95: null, max: null };
	}
	return {
		count: values.length,
		min: Math.min(...values),
		p50: quantile(values, 0.5),
		p95: quantile(values, 0.95),
		max: Math.max(...values),
	};
}

function quantile(values: number[], q: number): number {
	if (values.length === 0) return 0;
	const sorted = [...values].sort((a, b) => a - b);
	const index = (sorted.length - 1) * q;
	const lower = Math.floor(index);
	const fraction = index - lower;
	return sorted[lower] + (sorted[lower + 1] - sorted[lower] || 0) * fraction;
}

function ratio(numerator: number, denominator: number): number | null {
	return denominator === 0 ? null : numerator / denominator;
}
