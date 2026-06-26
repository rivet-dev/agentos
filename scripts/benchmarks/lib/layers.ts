import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { AgentOs } from "@rivet-dev/agentos-core";
import type { NativeOp } from "./native.js";
import { runNativeLayer } from "./native.js";
import { nowMs, round, stats, type Stats } from "./perf-utils.js";

export interface LayerSamples {
	native: number[];
	node: number[];
	guest: number[];
}

export interface LayerStats {
	native: Stats;
	node: Stats;
	guest: Stats;
}

export interface BenchmarkOp {
	family: string;
	name: string;
	nativeOp: NativeOp;
	fileLine: string;
	reproducer: string;
	expectedRatio?: "control";
	setup?: string;
	runNode?: (iters: number, warmup: number) => Promise<number[]> | number[];
	runGuest?: (
		vm: AgentOs,
		iters: number,
		warmup: number,
	) => Promise<number[]>;
	program?: string;
}

export interface OpResult {
	family: string;
	op: string;
	fileLine: string;
	reproducer: string;
	expectedRatio?: "control";
	layers: LayerStats;
	tax: {
		emulation: number;
		total: number;
	};
}

export function timedProgram(operationSource: string, setupSource?: string): string {
	return `
const iters = Number(process.env.BENCH_ITERATIONS || 20);
const warmup = Number(process.env.BENCH_WARMUP || 5);
const samples = [];
const now = () => Number(process.hrtime.bigint()) / 1e6;
const setup = ${setupSource ?? "null"};
const op = ${operationSource};
(async () => {
  if (typeof setup === "function") await setup();
  for (let i = 0; i < warmup + iters; i++) {
    const start = now();
    await op(i);
    const ms = now() - start;
    if (i >= warmup) samples.push(ms);
  }
  process.stdout.write(JSON.stringify({ samples }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : error);
  process.exit(1);
});
`;
}

export function runNodeProgram(
	source: string,
	iters: number,
	warmup: number,
): number[] {
	const dir = mkdtempSync(join(tmpdir(), "agentos-fuzz-perf-node-"));
	const file = join(dir, "bench.mjs");
	try {
		writeFileSync(file, source);
		const stdout = execFileSync("node", [file], {
			encoding: "utf8",
			env: {
				...process.env,
				BENCH_ITERATIONS: String(iters),
				BENCH_WARMUP: String(warmup),
			},
			maxBuffer: 128 * 1024 * 1024,
		});
		return JSON.parse(stdout).samples;
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
}

export async function runGuestProgram(
	vm: AgentOs,
	source: string,
	iters: number,
	warmup: number,
	name: string,
): Promise<number[]> {
	const path = `/tmp/fuzz-perf-${name.replace(/[^a-z0-9_-]/gi, "_")}.mjs`;
	await vm.writeFile(path, source);
	let stdout = "";
	let stderr = "";
	const proc = vm.spawn("node", [path], {
		env: {
			BENCH_ITERATIONS: String(iters),
			BENCH_WARMUP: String(warmup),
		},
		onStdout: (data) => {
			stdout += Buffer.from(data).toString("utf8");
		},
		onStderr: (data) => {
			stderr += Buffer.from(data).toString("utf8");
		},
	});
	const code = await vm.waitProcess(proc.pid);
	if (code !== 0) {
		throw new Error(`guest program ${name} exited ${code}\n${stderr}`);
	}
	return JSON.parse(stdout).samples;
}

export function runNodeSpawn(
	args: string[],
	iters: number,
	warmup: number,
): number[] {
	const samples: number[] = [];
	for (let i = 0; i < warmup + iters; i++) {
		const start = process.hrtime.bigint();
		const result = spawnSync("node", args, { stdio: "ignore" });
		const ms = nowMs(start);
		if (result.status !== 0) {
			throw new Error(`node spawn exited ${result.status}`);
		}
		if (i >= warmup) samples.push(ms);
	}
	return samples;
}

export async function runGuestSpawn(
	vm: AgentOs,
	args: string[],
	iters: number,
	warmup: number,
): Promise<number[]> {
	const samples: number[] = [];
	for (let i = 0; i < warmup + iters; i++) {
		const start = process.hrtime.bigint();
		const proc = vm.spawn("node", args);
		const code = await vm.waitProcess(proc.pid);
		const ms = nowMs(start);
		if (code !== 0) throw new Error(`guest spawn exited ${code}`);
		if (i >= warmup) samples.push(ms);
	}
	return samples;
}

export async function runOp(
	op: BenchmarkOp,
	vm: AgentOs,
	iters: number,
	warmup: number,
): Promise<OpResult> {
	const native = runNativeLayer(op.nativeOp, iters, warmup);
	const node = op.runNode
		? await op.runNode(iters, warmup)
		: runNodeProgram(timedProgram(op.program ?? "() => {}", op.setup), iters, warmup);
	const guest = op.runGuest
		? await op.runGuest(vm, iters, warmup)
		: await runGuestProgram(
				vm,
				timedProgram(op.program ?? "() => {}", op.setup),
				iters,
				warmup,
				`${op.family}-${op.name}`,
			);
	const layers = {
		native: stats(native),
		node: stats(node),
		guest: stats(guest),
	};
	return {
		family: op.family,
		op: op.name,
		fileLine: op.fileLine,
		reproducer: op.reproducer,
		expectedRatio: op.expectedRatio,
		layers,
		tax: {
			emulation: round(layers.guest.p50 / layers.node.p50),
			total: round(layers.guest.p50 / layers.native.p50),
		},
	};
}
