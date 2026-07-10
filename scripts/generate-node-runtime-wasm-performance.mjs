#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputPath = resolve(repoRoot, "docs-internal/node-runtime-wasm-performance.json");
const arguments_ = process.argv.slice(2);
const check = arguments_.includes("--check");
const requireResults = arguments_.includes("--require-results");
if (arguments_.some((argument) => !["--check", "--require-results"].includes(argument))) {
	throw new Error("usage: generate-node-runtime-wasm-performance.mjs [--check] [--require-results]");
}

const cases = [
	["cold-compile", "startup/cold-compile.mjs", "ms", true, 30, 5],
	["clean-bootstrap", "startup/clean-bootstrap.mjs", "ms", true, 30, 5],
	["cached-bootstrap", "startup/cached-bootstrap.mjs", "ms", true, 30, 5],
	["cjs-import-storm", "modules/cjs-import-storm.cjs", "ms", true, 30, 5],
	["esm-import-storm", "modules/esm-import-storm.mjs", "ms", true, 30, 5],
	["fs-read-4k", "fs/read.mjs?bytes=4096", "MiB/s", false, 30, 5],
	["fs-read-1m", "fs/read.mjs?bytes=1048576", "MiB/s", false, 30, 5],
	["fs-write-4k", "fs/write.mjs?bytes=4096", "MiB/s", false, 30, 5],
	["fs-write-1m", "fs/write.mjs?bytes=1048576", "MiB/s", false, 30, 5],
	["fs-stat-readdir-storm", "fs/stat-readdir-storm.mjs", "ops/s", false, 30, 5],
	["fs-stream-copy", "fs/stream-copy.mjs", "MiB/s", false, 30, 5],
	["tcp-stream-1b", "net/tcp-stream.mjs?chunk=1", "MiB/s", false, 30, 5],
	["tcp-stream-16k", "net/tcp-stream.mjs?chunk=16384", "MiB/s", false, 30, 5],
	["http-stream-1b", "net/http-stream.mjs?chunk=1", "MiB/s", false, 30, 5],
	["http-stream-16k", "net/http-stream.mjs?chunk=16384", "MiB/s", false, 30, 5],
	["http-request-throughput", "net/http-throughput.mjs", "requests/s", false, 30, 5],
	["http-backpressure", "net/http-backpressure.mjs", "ms", true, 30, 5],
	["tls-handshake", "tls/handshake.mjs", "handshakes/s", false, 30, 5],
	["tls-resumption", "tls/resumption.mjs", "handshakes/s", false, 30, 5],
	["tls-small-record", "tls/small-record.mjs", "MiB/s", false, 30, 5],
	["crypto-sha256", "crypto/sha256.mjs", "MiB/s", false, 30, 5],
	["crypto-aes-gcm", "crypto/aes-gcm.mjs", "MiB/s", false, 30, 5],
	["crypto-rsa", "crypto/rsa.mjs", "ops/s", false, 30, 5],
	["crypto-kdf", "crypto/kdf.mjs", "ops/s", false, 30, 5],
	["compression-zlib", "compression/zlib.mjs", "MiB/s", false, 30, 5],
	["compression-brotli", "compression/brotli.mjs", "MiB/s", false, 30, 5],
	["compression-zstd", "compression/zstd.mjs", "MiB/s", false, 30, 5],
	["napi-scalar", "napi/scalar.mjs", "ns/op", true, 30, 5],
	["napi-property", "napi/property.mjs", "ns/op", true, 30, 5],
	["napi-callback", "napi/callback.mjs", "ns/op", true, 30, 5],
	["napi-buffer-copy", "napi/buffer-copy.mjs", "MiB/s", false, 30, 5],
	["napi-buffer-pinned", "napi/buffer-pinned.mjs", "MiB/s", false, 30, 5],
	["idle-runtime-rss", "memory/idle-runtime.mjs", "bytes", true, 20, 3],
	["concurrent-vm-rss", "memory/concurrent-vms.mjs", "bytes/vm", true, 20, 3],
];

const implementations = {
	nativeNode: {
		source: "pinned upstream Node v24.15.0 native Linux release build",
		revision: "v24.15.0",
	},
	legacyAgentos: {
		source: "frozen pre-refactor AgentOS implementation at main",
		revision: "0faec6268dd8b3d127234f8f76b90c9b143dd938",
	},
	nodeRuntimeWasm: {
		source: "candidate node-runtime.wasm in the existing AgentOS V8 stack",
		revision: "candidate",
	},
};

function resultSlot() {
	return {
		status: "required-not-measured",
		absolute: null,
		p50: null,
		p95: null,
		p99: null,
		iqr: null,
		min: null,
		max: null,
		samples: [],
	};
}

let existingRows = new Map();
if (existsSync(outputPath)) {
	const existing = JSON.parse(readFileSync(outputPath, "utf8"));
	existingRows = new Map((existing.rows ?? []).map((row) => [row.id, row]));
}

const rows = cases.map(([id, fixture, unit, lowerIsBetter, samples, warmup]) => ({
	id,
	fixture: `scripts/benchmarks/node-runtime-wasm-fixtures/${fixture}`,
	unit,
	lowerIsBetter,
	buildProfile: "release-lto-pinned",
	warmup,
	samples,
	statistic: "p99",
	noiseRule: "rerun all three lanes when either baseline repeat differs by more than 5%; use the slower baseline run",
	thresholds: {
		maximumRegressionVersusLegacyPercent: 10,
		missingComparatorFails: true,
		percentageOnlyFails: true,
	},
	commands: Object.fromEntries(Object.keys(implementations).map((implementation) => [
		implementation,
		`node scripts/run-node-runtime-wasm-benchmark.mjs --case ${id} --implementation ${implementation} --warmup ${warmup} --samples ${samples} --profile release-lto-pinned`,
	])),
	results: existingRows.get(id)?.results ?? {
		nativeNode: resultSlot(),
		legacyAgentos: resultSlot(),
		nodeRuntimeWasm: resultSlot(),
		newVersusNative: { absoluteDelta: null, ratio: null, percent: null },
		newVersusLegacy: { absoluteDelta: null, ratio: null, percent: null },
	},
}));

const allResultsMeasured = rows.every((row) =>
	["nativeNode", "legacyAgentos", "nodeRuntimeWasm"].every((implementation) =>
		row.results[implementation]?.status === "measured" &&
		Number.isFinite(row.results[implementation]?.absolute) &&
		row.results[implementation]?.samples?.length === row.samples,
	),
);

const output = `${JSON.stringify({
	schema: 1,
	status: allResultsMeasured ? "measured-pending-threshold-evaluation" : "r0-baselines-required",
	purpose: "Frozen three-way before/after benchmark contract. Null result slots are failing, never skipped or passing evidence.",
	implementations,
	hostProtocol: {
		os: "Linux",
		architecture: "x86_64",
		cpuGovernor: "performance",
		samePhysicalHostRequired: true,
		noConcurrentWorkloads: true,
		record: ["cpuModel", "logicalCpuCount", "memoryBytes", "kernel", "compiler", "backend", "moduleSha256"],
	},
	concurrencyGates: {
		offenderTerminationCeilingMs: 5000,
		controlVmAbsoluteP99CeilingMs: 50,
		controlVmLoadedToUnloadedP99MaximumRatio: 2,
	},
	rows,
}, null, 2)}\n`;

if (check) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== output) {
		throw new Error(`generated Node runtime performance contract is stale: ${outputPath}`);
	}
} else {
	writeFileSync(outputPath, output);
}

if (requireResults) {
	const measured = JSON.parse(readFileSync(outputPath, "utf8"));
	const missing = measured.rows.flatMap((row) =>
		["nativeNode", "legacyAgentos", "nodeRuntimeWasm"]
			.filter((implementation) =>
				row.results[implementation].status !== "measured" ||
				!Number.isFinite(row.results[implementation].absolute) ||
				row.results[implementation].samples.length !== row.samples,
			)
			.map((implementation) => `${row.id}:${implementation}`),
	);
	if (missing.length > 0) {
		throw new Error(`Node runtime performance evidence is incomplete (${missing.length} lanes): ${missing.slice(0, 8).join(", ")}${missing.length > 8 ? ", ..." : ""}`);
	}
}

process.stdout.write(
	`Node runtime WASM three-way performance contract ${check ? "verified" : "generated"}: ${rows.length} required rows, ${rows.length * 3} required result lanes\n`,
);
