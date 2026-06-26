import { execFileSync } from "node:child_process";

const DEFAULT_NATIVE_BIN =
	"/home/nathan/.herdr/workspaces/secure-exec/fuzz-perf/target/release/native-baseline";

export type NativeOp =
	| "spawn_exit"
	| "exec_capture"
	| "node_stdout_discard_2b"
	| "node_stdout_capture_2b"
	| "node_stdout_listener_only_2b"
	| "node_exit"
	| "node_fanout"
	| "node_reap_storm"
	| "pipe_chain"
	| "fs_stat"
	| "fs_write"
	| "fs_read"
	| "fs_open_close"
	| "fs_mkdir_rmdir"
	| "fs_rename"
	| "fs_readdir"
	| "fs_fsync"
	| "dns_lookup"
	| "dns_concurrent"
	| "tcp_connect"
	| "tcp_echo"
	| "tcp_concurrent"
	| "tcp_throughput"
	| "tcp_tiny_writes"
	| "udp_echo"
	| "pipe_echo"
	| "pipe_throughput"
	| "pipe_backpressure"
	| "cpu_loop"
	| "alloc_free";

export function runNativeLayer(
	op: NativeOp,
	iters: number,
	warmup: number,
): number[] {
	const bin = process.env.NATIVE_BASELINE_BIN ?? DEFAULT_NATIVE_BIN;
	const stdout = execFileSync(
		bin,
		["--op", op, "--iters", String(iters), "--warmup", String(warmup)],
		{ encoding: "utf8", maxBuffer: 128 * 1024 * 1024 },
	);
	const parsed = JSON.parse(stdout) as {
		unit: string;
		samples: number[];
	};
	if (parsed.unit !== "ns") {
		throw new Error(`native-baseline emitted unexpected unit: ${parsed.unit}`);
	}
	return parsed.samples.map((ns) => ns / 1e6);
}
