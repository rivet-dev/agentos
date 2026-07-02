import { execFileSync, spawnSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdtempSync,
	mkdirSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { AgentOs, createHostDirBackend } from "@rivet-dev/agentos-core";
import type { NativeOp } from "./native.js";
import { runNativeLayer } from "./native.js";
import { nowMs, round, stats, type Stats } from "./perf-utils.js";

const DEFAULT_NATIVE_BASELINE_WASM =
	"/home/nathan/.herdr/workspaces/agent-os/secure-exec-perf-rules/target/wasm32-wasip1/release/native-baseline.wasm";
const WASM_COMMAND_NAME = "native-baseline";
const WASM_COMMAND_PATH = `/__secure_exec/commands/0/${WASM_COMMAND_NAME}`;
const WASM_BASE_DIR = "/mnt/native-baseline-wasm";
const WASM_SUPPORTED_OPS = new Set<NativeOp>([
	"fs_stat",
	"fs_write",
	"fs_read",
	"fs_open_close",
	"fs_mkdir_rmdir",
	"fs_rename",
	"fs_readdir",
	"fs_fsync",
	"cpu_loop",
	"alloc_free",
]);
let wasmCommandDir: string | undefined;
let wasmWritableDir: string | undefined;

export interface LayerSamples {
	native: number[];
	node: number[];
	guest: number[];
	wasm?: number[];
}

export interface LayerStats {
	native: Stats;
	node: Stats;
	guest: Stats;
	wasm?: Stats;
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
		wasm?: number;
	};
}

export function supportsWasmLayer(op: NativeOp): boolean {
	return WASM_SUPPORTED_OPS.has(op);
}

export function wasmLayerMounts():
	| { path: string; plugin: ReturnType<typeof createHostDirBackend> }[]
	| undefined {
	const wasm = resolveNativeBaselineWasm();
	if (!wasm) return undefined;
	return [
		{
			path: "/__secure_exec/commands/0",
			plugin: createHostDirBackend({
				hostPath: ensureWasmCommandDir(wasm),
				readOnly: true,
			}),
		},
		{
			path: WASM_BASE_DIR,
			plugin: createHostDirBackend({
				hostPath: ensureWasmWritableDir(),
				readOnly: false,
			}),
		},
	];
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

function resolveNativeBaselineWasm(): string | undefined {
	const wasm = process.env.NATIVE_BASELINE_WASM ?? DEFAULT_NATIVE_BASELINE_WASM;
	if (!wasm || !existsSync(wasm)) return undefined;
	return wasm;
}

function ensureWasmCommandDir(wasmPath: string): string {
	if (wasmCommandDir) return wasmCommandDir;
	const dir = mkdtempSync(join(tmpdir(), "agentos-native-baseline-wasm-cmd-"));
	mkdirSync(dir, { recursive: true });
	copyFileSync(wasmPath, join(dir, WASM_COMMAND_NAME));
	wasmCommandDir = dir;
	return wasmCommandDir;
}

function ensureWasmWritableDir(): string {
	if (wasmWritableDir) return wasmWritableDir;
	wasmWritableDir = mkdtempSync(join(tmpdir(), "agentos-native-baseline-wasm-data-"));
	return wasmWritableDir;
}

export async function runWasmLayer(
	vm: AgentOs,
	nativeOp: NativeOp,
	iters: number,
	warmup: number,
): Promise<number[] | undefined> {
	if (!supportsWasmLayer(nativeOp)) return undefined;
	if (!resolveNativeBaselineWasm()) return undefined;
	const hostBaseDir = join(ensureWasmWritableDir(), nativeOp);
	rmSync(hostBaseDir, { recursive: true, force: true });
	mkdirSync(hostBaseDir, { recursive: true });
	const guestBaseDir = `${WASM_BASE_DIR}/${nativeOp}`;
	const result = await vm.execArgv(WASM_COMMAND_PATH, [
		"--op",
		nativeOp,
		"--iters",
		String(iters),
		"--warmup",
		String(warmup),
		"--base-dir",
		guestBaseDir,
	]);
	if (result.exitCode !== 0) {
		throw new Error(`wasm native-baseline ${nativeOp} exited ${result.exitCode}\n${result.stderr}`);
	}
	const parsed = JSON.parse(result.stdout) as {
		unit?: string;
		samples?: number[];
		unsupported?: boolean;
		op?: string;
	};
	if (parsed.unsupported) {
		throw new Error(`wasm native-baseline unexpectedly returned unsupported for ${nativeOp}`);
	}
	if (parsed.unit !== "ns" || !Array.isArray(parsed.samples)) {
		throw new Error(`wasm native-baseline emitted unexpected output: ${result.stdout}`);
	}
	return parsed.samples.map((ns) => ns / 1e6);
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
	const wasm = await runWasmLayer(vm, op.nativeOp, iters, warmup);
	const layers = {
		native: stats(native),
		node: stats(node),
		guest: stats(guest),
		...(wasm ? { wasm: stats(wasm) } : {}),
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
			...(layers.wasm ? { wasm: round(layers.wasm.p50 / layers.native.p50) } : {}),
		},
	};
}
