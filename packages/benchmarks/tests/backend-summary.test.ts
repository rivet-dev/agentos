import assert from "node:assert/strict";
import { test } from "node:test";
import { summarizeBackendComparison } from "../src/lib/backend-summary.js";

function fixture(wasmtimeSuccessful: number) {
	const memory = {
		rssBytes: 100,
		pssBytes: 80,
		peakRssBytes: 100,
		virtualBytes: 100,
		minorFaults: 0,
		majorFaults: 0,
	};
	return {
		metadata: {
			freshProcesses: 5,
			samplesPerProcess: 5,
			concurrencyLevels: [50],
		},
		fresh: ["v8", "wasmtime"].flatMap((backend) =>
			Array.from({ length: 5 }, (_, processIndex) => ({
				backend,
				processIndex,
				retained: memory,
				retainedDelta: memory,
				workloads: [
					"trivial",
					"coreutils",
					"shell",
					"curl",
					"sqlite",
					"vim",
					"large-module",
					"compute-heavy",
					"host-call-heavy",
				].map((name) => ({
					name,
					samples: Array.from({ length: 5 }, (_, index) => ({
						durationMs: 1,
						cacheState: index === 0 ? "fresh" : "warm",
						passed: true,
					})),
				})),
			})),
		),
		paths: ["v8", "wasmtime"].map((backend) => ({
			backend,
			denial: { passed: true },
			cancellation: { passed: true },
			resourceLimit: { passed: true },
		})),
		concurrency: ["v8", "wasmtime"].map((backend) => ({
			backend,
			levels: ["repeated", "diverse"].map((mode) => ({
				level: 50,
				mode,
				successful: backend === "v8" ? 50 : wasmtimeSuccessful,
				throughputPerSecond: backend === "v8" ? 100 : 200,
				failedExitCodes: backend === "v8" ? 0 : 50 - wasmtimeSuccessful,
				rejectedCount: 0,
				failureExamples:
					backend === "wasmtime" && wasmtimeSuccessful < 50
						? ["ERR_AGENTOS_VM_EXECUTOR_LIMIT: maxActiveVms=20"]
						: [],
				rejectionExamples: [],
			})),
		})),
	};
}

test("higher successful throughput cannot hide serving fewer requests", () => {
	const summary = summarizeBackendComparison(fixture(20));
	assert.equal(summary.gates.throughput, false);
});

test("matched all-success load passes the throughput gate", () => {
	assert.equal(summarizeBackendComparison(fixture(50)).gates.throughput, true);
});

test("summary preserves admission outcomes and does not mutate observations", () => {
	const raw = fixture(20);
	const before = structuredClone(raw);
	const summary = summarizeBackendComparison(raw);
	const row = summary.throughput[0];
	assert.equal(row.admissionParity, false);
	assert.equal(row.admissions.v8.successful, 50);
	assert.equal(row.admissions.wasmtime?.successful, 20);
	assert.equal(row.admissions.wasmtime?.failedExitCodes, 30);
	assert.deepEqual(row.admissions.wasmtime?.failureExamples, [
		"ERR_AGENTOS_VM_EXECUTOR_LIMIT: maxActiveVms=20",
	]);
	assert.deepEqual(raw, before);
});

test("missing backend and empty concurrency cannot pass throughput", () => {
	const raw = fixture(50);
	raw.concurrency.pop();
	assert.equal(summarizeBackendComparison(raw).gates.throughput, false);
	raw.concurrency = [];
	assert.equal(summarizeBackendComparison(raw).gates.throughput, false);
});

test("omitted expected workload in both backends fails correctness", () => {
	const raw = fixture(50);
	assert.equal(summarizeBackendComparison(raw).gates.correctness, true);
	for (const run of raw.fresh)
		run.workloads = run.workloads.filter((w) => w.name !== "curl");
	assert.equal(summarizeBackendComparison(raw).gates.correctness, false);
});

test("missing runs, samples, paths, and concurrency rows fail completeness", () => {
	const mutations: Array<(raw: ReturnType<typeof fixture>) => void> = [
		(raw) => {
			raw.fresh.pop();
		},
		(raw) => {
			raw.fresh[0].workloads[0].samples.pop();
		},
		(raw) => {
			raw.paths = [];
		},
		(raw) => {
			raw.paths.pop();
		},
		(raw) => {
			raw.concurrency = [];
		},
		(raw) => {
			raw.concurrency.pop();
		},
		(raw) => {
			raw.concurrency[0].levels.pop();
		},
		(raw) => {
			raw.fresh[1].processIndex = raw.fresh[0].processIndex;
		},
	];
	for (const mutate of mutations) {
		const raw = fixture(50);
		mutate(raw);
		assert.equal(summarizeBackendComparison(raw).gates.correctness, false);
	}
});

test("both backends serving zero requests cannot pass throughput", () => {
	const raw = fixture(0);
	for (const backend of raw.concurrency)
		for (const row of backend.levels) {
			row.successful = 0;
			row.failedExitCodes = 50;
			row.throughputPerSecond = 0;
		}
	assert.equal(summarizeBackendComparison(raw).gates.throughput, false);
});

test("absent raw sections and run metadata fail closed", () => {
	for (const key of ["fresh", "paths", "concurrency", "metadata"]) {
		const raw: Record<string, unknown> = fixture(50);
		delete raw[key];
		const summary = summarizeBackendComparison(raw);
		assert.equal(summary.completeness.passed, false, key);
		assert.equal(summary.gates.correctness, false, key);
		assert.equal(summary.gates.throughput, false, key);
	}
});
