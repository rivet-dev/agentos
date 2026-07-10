import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { getHardware, percentile, round } from "../lib/perf-utils.js";
import { formatPacificIso } from "../lib/vm.js";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const iterations = Math.max(
	5,
	Number(process.env.BENCH_NODE_STDLIB_ITERATIONS ?? 9),
);
const warmup = Math.max(1, Number(process.env.BENCH_NODE_STDLIB_WARMUP ?? 3));
const runs = Math.max(5, Number(process.env.BENCH_NODE_STDLIB_RUNS ?? 5));
const POST_TRANSPORT_BUDGET_MS = new Map([
	["fs_read_small", 0.03],
	["fs_read_big", 0.42],
]);

function lane(
	name: "base64-fd" | "backing-store-fd" | "optimized-read-file",
	transport: "base64" | "backing-store",
	readFileFastPath: boolean,
) {
	const stdout = execFileSync(
		"pnpm",
		[
			"--silent",
			"--dir",
			packageRoot,
			"exec",
			"tsx",
			resolve(packageRoot, "src/run-all.ts"),
		],
		{
			cwd: packageRoot,
			env: {
				...process.env,
				AGENTOS_JS_STDLIB: "real",
				AGENTOS_BENCH_FS_SYNC_READ_TRANSPORT: transport,
				AGENTOS_BENCH_FS_READFILE_FAST_PATH: readFileFastPath ? "1" : "0",
				BENCH_ITERATIONS: String(iterations),
				BENCH_WARMUP: String(warmup),
				BENCH_SHARED_VM: "1",
				BENCH_NO_WRITE: "1",
				BENCH_FAMILIES: "fs",
				BENCH_OP_FILTER: "fs_read_small,fs_read_big",
			},
			encoding: "utf8",
			maxBuffer: 128 * 1024 * 1024,
			stdio: ["ignore", "pipe", "inherit"],
		},
	);
	const report = JSON.parse(stdout);
	return {
		name,
		transport,
		readFileFastPath,
		rows: report.latency.map((row: any) => ({
			op: row.op,
			payloadBytes: row.payloadBytes,
			p50Ms: row.layers.guest.p50,
			p99Ms: row.layers.guest.p99,
			minMs: row.layers.guest.min,
			maxMs: row.layers.guest.max,
		})),
	};
}

function aggregate(name: string, laneRuns: Array<ReturnType<typeof lane>>) {
	const operations = laneRuns[0].rows.map((row: any) => row.op);
	return {
		name,
		transport: laneRuns[0].transport,
		readFileFastPath: laneRuns[0].readFileFastPath,
		runs: laneRuns.map((run) => run.rows),
		rows: operations.map((op: string) => {
			const rows = laneRuns.map((run) =>
				run.rows.find((row: any) => row.op === op),
			);
			const p50s = rows
				.map((row: any) => row.p50Ms)
				.sort((a: number, b: number) => a - b);
			const p99s = rows
				.map((row: any) => row.p99Ms)
				.sort((a: number, b: number) => a - b);
			return {
				op,
				p50Ms: percentile(p50s, 50),
				p99Ms: p99s.at(-1),
				p50IqrMs: round(percentile(p50s, 75) - percentile(p50s, 25), 4),
				runP50Ms: p50s,
				runP99Ms: p99s,
			};
		}),
	};
}

const base64Runs: Array<ReturnType<typeof lane>> = [];
const backingStoreRuns: Array<ReturnType<typeof lane>> = [];
const optimizedReadFileRuns: Array<ReturnType<typeof lane>> = [];
for (let run = 0; run < runs; run++) {
	const first = run % 2 === 0 ? base64Runs : backingStoreRuns;
	const second = run % 2 === 0 ? backingStoreRuns : base64Runs;
	first.push(
		run % 2 === 0
			? lane("base64-fd", "base64", false)
			: lane("backing-store-fd", "backing-store", false),
	);
	second.push(
		run % 2 === 0
			? lane("backing-store-fd", "backing-store", false)
			: lane("base64-fd", "base64", false),
	);
	optimizedReadFileRuns.push(
		lane("optimized-read-file", "backing-store", true),
	);
}
const base64 = aggregate("base64-fd", base64Runs);
const backingStore = aggregate("backing-store-fd", backingStoreRuns);
const optimizedReadFile = aggregate(
	"optimized-read-file",
	optimizedReadFileRuns,
);
const base64Rows = new Map(base64.rows.map((row: any) => [row.op, row]));

console.log(
	JSON.stringify(
		{
			schema: 1,
			generatedAt: formatPacificIso(new Date()),
			hardware: getHardware(),
			protocol: {
				iterations,
				warmup,
				runs,
				stdlib: "real",
				comparison:
					"fd-level CBOR/base64 response versus direct write into the guest backing store",
				budgetLane: "one-call raw-byte path-based readFileSync fast path",
			},
			lanes: { base64, backingStore, optimizedReadFile },
			transportDeltas: backingStore.rows.map((row: any) => {
				const before = base64Rows.get(row.op) as any;
				return {
					op: row.op,
					base64P50Ms: before.p50Ms,
					backingStoreP50Ms: row.p50Ms,
					ratio: round(row.p50Ms / before.p50Ms, 4),
					reductionPercent: round((1 - row.p50Ms / before.p50Ms) * 100, 2),
				};
			}),
			budgets: optimizedReadFile.rows.map((row: any) => {
				const budgetMs = POST_TRANSPORT_BUDGET_MS.get(row.op);
				return {
					op: row.op,
					optimizedP50Ms: row.p50Ms,
					budgetMs,
					budgetMet: budgetMs === undefined ? null : row.p50Ms <= budgetMs,
				};
			}),
		},
		null,
		2,
	),
);
