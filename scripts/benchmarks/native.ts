/**
 * Driver for the native-baseline Rust binary (the "native floor" layer).
 *
 * Spawns the binary, which performs a logical op N times and prints a JSON array of
 * raw per-iteration timings in nanoseconds. We convert to milliseconds and hand the
 * raw samples back so the caller can reduce them with the SAME stats() used for the
 * node and guest layers — identical percentile math across all three layers.
 */

import { execFileSync } from "node:child_process";

const DEFAULT_NATIVE_BIN =
	"/home/nathan/.herdr/workspaces/secure-exec/fuzz-perf/target/release/native-baseline";

export type NativeOp = "spawn_exit" | "exec_capture" | "node_exit" | "node_fanout";

/** Run the native layer for one op. Returns raw per-iteration samples in milliseconds. */
export function runNativeLayer(
	op: NativeOp,
	iters: number,
	warmup: number,
): number[] {
	const bin = process.env.NATIVE_BASELINE_BIN ?? DEFAULT_NATIVE_BIN;
	const stdout = execFileSync(
		bin,
		["--op", op, "--iters", String(iters), "--warmup", String(warmup)],
		{ encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
	);
	const parsed = JSON.parse(stdout) as {
		layer: string;
		op: string;
		unit: string;
		samples: number[];
	};
	if (parsed.unit !== "ns") {
		throw new Error(`native-baseline emitted unexpected unit: ${parsed.unit}`);
	}
	// ns -> ms, matching the node/guest layers (process.hrtime.bigint() ns -> ms).
	return parsed.samples.map((ns) => ns / 1e6);
}

export type NativePhaseSamples = Record<string, number[]>;

/** Run a native phase-enabled op. Returns named sample arrays in milliseconds. */
export function runNativePhaseLayer(
	op: Extract<NativeOp, "node_exit" | "node_fanout">,
	iters: number,
	warmup: number,
): NativePhaseSamples {
	const bin = process.env.NATIVE_BASELINE_BIN ?? DEFAULT_NATIVE_BIN;
	const stdout = execFileSync(
		bin,
		[
			"--op",
			op,
			"--iters",
			String(iters),
			"--warmup",
			String(warmup),
			"--phases",
		],
		{ encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
	);
	const parsed = JSON.parse(stdout) as {
		layer: string;
		op: string;
		unit: string;
		phases: Record<string, number[]>;
	};
	if (parsed.unit !== "ns") {
		throw new Error(`native-baseline emitted unexpected unit: ${parsed.unit}`);
	}
	return Object.fromEntries(
		Object.entries(parsed.phases).map(([phase, samples]) => [
			phase,
			samples.map((ns) => ns / 1e6),
		]),
	);
}
