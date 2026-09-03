import {
	AgentOs,
	type AgentOsOptions,
	type AgentOsSidecar,
	type RootSnapshotExport,
} from "@rivet-dev/agentos-core";
import { coreutils } from "@agentos-software/common";
import os from "node:os";

// Benchmark parameters. Keep batch sizes minimal for fast iteration.
export const ITERATIONS = 5;
export const WARMUP_ITERATIONS = 1;

export const ECHO_COMMAND = "echo hello";
export const EXPECTED_OUTPUT = "hello\n";
// ── Shared bench sidecar + cold-run snapshot ───────────────────────
//
// Benchmarks create the sidecar ONCE up front and lease every VM from it,
// rather than letting each `AgentOs.create()` stand up its own sidecar. A
// single cold-run VM is then created and its root filesystem snapshotted so
// the subsequent measured VMs boot from a warm snapshot instead of paying the
// bootstrap cost on every iteration. Both are wired through `benchCreateOptions`
// so every VM-creation helper picks them up automatically.

let _benchSidecar: AgentOsSidecar | undefined;
let _benchSnapshot: RootSnapshotExport | undefined;

/** Create the shared bench sidecar. Call once before creating any VM. */
export async function startBenchSidecar(): Promise<AgentOsSidecar> {
	if (!_benchSidecar) {
		_benchSidecar = await AgentOs.createSidecar();
	}
	return _benchSidecar;
}

/** Dispose the shared bench sidecar and clear the cold-run snapshot. */
export async function stopBenchSidecar(): Promise<void> {
	_benchSnapshot = undefined;
	if (_benchSidecar) {
		const sidecar = _benchSidecar;
		_benchSidecar = undefined;
		await sidecar.dispose();
	}
}

/** Record the cold-run root snapshot reused by subsequent measured VMs. */
export function setBenchRootSnapshot(snapshot: RootSnapshotExport): void {
	_benchSnapshot = snapshot;
}

/** Clear the cold-run root snapshot (e.g. when switching workloads). */
export function clearBenchRootSnapshot(): void {
	_benchSnapshot = undefined;
}

/**
 * Overlay the shared bench sidecar (and cold-run snapshot, when set) onto a
 * VM-creation options object. The snapshot only applies when the caller did
 * not request its own `rootFilesystem`.
 */
export function benchCreateOptions(options: AgentOsOptions = {}): AgentOsOptions {
	const overlay: AgentOsOptions = { ...options };
	if (_benchSidecar) {
		overlay.sidecar = { kind: "explicit", handle: _benchSidecar };
	}
	if (_benchSnapshot && overlay.rootFilesystem === undefined) {
		overlay.rootFilesystem = { type: "overlay", lowers: [_benchSnapshot] };
	}
	return overlay;
}

// ── Workload abstraction ────────────────────────────────────────────

export interface WorkloadObservation {
	workloadPath?: string;
}

/** A workload describes how to create a VM and start a long-running process for memory measurement. */
export interface Workload {
	name: string;
	description: string;
	createVm: () => Promise<AgentOs>;
	/** Start a long-running process so the Worker thread stays alive. */
	start: (vm: AgentOs) => Promise<WorkloadObservation | void> | WorkloadObservation | void;
	/** Verify the expected processes are running. Throws if not. */
	verify: (vm: AgentOs) => void;
	/** Time to wait after start for the process to fully initialize. */
	settleMs: number;
}

export const WORKLOADS: Record<string, Workload> = {
	sleep: {
		name: "sleep",
		description: "Minimal VM with idle Node.js process (setTimeout keepalive)",
		createVm: () => AgentOs.create(benchCreateOptions()),
		start: (vm) => {
			vm.spawn("node", ["-e", "setTimeout(() => {}, 999999999)"], {
				streamStdin: true,
			});
		},
		verify: (vm) => {
			const procs = vm.listProcesses();
			const running = procs.filter((p) => p.running);
			const hasNode = running.some((p) => p.command === "node");
			if (!hasNode) {
				throw new Error(
					`Expected running 'node' process, got: ${JSON.stringify(running.map((p) => p.command))}`,
				);
			}
		},
		settleMs: 2000,
	},
};

// ── VM creation helpers ─────────────────────────────────────────────

/**
 * Create a fresh agentOS VM with only coreutils (WASM shell + echo).
 * This is the minimal setup needed to run shell commands.
 */
export async function createBenchVm(): Promise<AgentOs> {
	return AgentOs.create(
		benchCreateOptions({
			software: [coreutils],
		}),
	);
}

// ── Stats and formatting ────────────────────────────────────────────

export function percentile(sorted: number[], p: number): number {
	const idx = Math.ceil((p / 100) * sorted.length) - 1;
	return sorted[Math.max(0, idx)];
}

export function stats(samples: number[]) {
	const sorted = [...samples].sort((a, b) => a - b);
	const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
	return {
		mean: round(mean),
		p50: round(percentile(sorted, 50)),
		p95: round(percentile(sorted, 95)),
		p99: round(percentile(sorted, 99)),
		min: round(sorted[0]),
		max: round(sorted[sorted.length - 1]),
	};
}

export function round(n: number, decimals = 2): number {
	const f = 10 ** decimals;
	return Math.round(n * f) / f;
}

export function getHardware() {
	const cpus = os.cpus();
	return {
		cpu: cpus[0]?.model ?? "unknown",
		cores: os.availableParallelism(),
		ram: `${round(os.totalmem() / 1024 ** 3, 1)} GB`,
		node: process.version,
		os: `${os.type()} ${os.release()}`,
		arch: os.arch(),
	};
}

export function forceGC() {
	if (global.gc) {
		global.gc();
	} else {
		console.error("WARNING: global.gc not available. Run with --expose-gc");
	}
}

export async function sleep(ms: number): Promise<void> {
	return new Promise((r) => setTimeout(r, ms));
}

export function formatBytes(bytes: number): string {
	if (Math.abs(bytes) < 1024) return `${bytes} B`;
	const mb = bytes / (1024 * 1024);
	return `${round(mb, 2)} MB`;
}

/** Print a table to stderr for human readability. */
export function printTable(
	headers: string[],
	rows: (string | number)[][],
): void {
	const widths = headers.map((h, i) =>
		Math.max(h.length, ...rows.map((r) => String(r[i]).length)),
	);
	const sep = widths.map((w) => "-".repeat(w)).join(" | ");
	const fmt = (row: (string | number)[]) =>
		row.map((c, i) => String(c).padStart(widths[i])).join(" | ");

	console.error("");
	console.error(fmt(headers));
	console.error(sep);
	for (const row of rows) {
		console.error(fmt(row));
	}
	console.error("");
}
