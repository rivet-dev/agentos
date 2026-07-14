import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
	AgentOs,
	type AgentOsOptions,
	type AgentOsSidecar,
} from "@rivet-dev/agentos-core";
import { resolvePublishedSidecarBinary } from "@rivet-dev/agentos-runtime-core/binary";
import { hasNativeBaselineWasm, supportsWasmLayer } from "./layers.js";
import type { BenchmarkOp, CommandBenchmarkOp } from "./layers.js";

const NATIVE_BASELINE_WASM_COMMAND = "native-baseline";
const NATIVE_BASELINE_WASM_PREWARM_DIR = "/mnt/native-baseline-wasm/prewarm";
const DEFAULT_COMMANDS_DIR = fileURLToPath(
	new URL("../../../runtime-core/commands/", import.meta.url),
);
const benchSidecarPids = new WeakMap<AgentOsSidecar, number>();

export interface BenchVmOptions {
	commandsDir?: string;
	loopbackExemptPorts?: number[];
	mounts?: HostDirectoryMount[];
	permissions?: AgentOsOptions["permissions"];
	wasmCommandDirs?: string[];
	sidecar?: AgentOsSidecar;
}

export interface HostDirectoryMount {
	guestPath: string;
	hostPath: string;
	readOnly?: boolean;
}

export type BenchSidecar = AgentOsSidecar;

export interface BenchVmProcess {
	pid: number;
	wait(): Promise<number>;
}

export interface BenchVm {
	writeFile(path: string, content: string | Uint8Array): Promise<void>;
	mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
	delete(path: string, options?: { recursive?: boolean }): Promise<void>;
	readFile(path: string): Promise<Uint8Array>;
	readDir(path: string): Promise<string[]>;
	readdir(path: string): Promise<string[]>;
	exec(
		commandLine: string,
		options?: {
			env?: Record<string, string>;
			cwd?: string;
			stdin?: string | Uint8Array;
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): Promise<{ stdout: string; stderr: string; exitCode: number }>;
	execArgv(
		command: string,
		args: string[],
		options?: {
			env?: Record<string, string>;
			cwd?: string;
			stdin?: string | Uint8Array;
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): Promise<{ stdout: string; stderr: string; exitCode: number }>;
	spawnNodeCapture(
		argsOrProgramPath: string[] | string,
		env?: Record<string, string>,
		options?: {
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): Promise<{ stdout: string; stderr: string; exitCode: number }>;
	spawn(
		command: string,
		args: string[],
		options?: {
			env?: Record<string, string>;
			cwd?: string;
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): Promise<BenchVmProcess>;
	waitProcess(pid: number): Promise<number>;
	execWasmCommand(
		cmd: string,
		args: string[],
		options?: {
			env?: Record<string, string>;
			cwd?: string;
			stdin?: string | Uint8Array;
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): Promise<{ stdout: string; stderr: string; exitCode: number }>;
	dispose(): Promise<void>;
	sidecarPid(): number | null;
}

export interface SidecarBinaryProvenance {
	path: string;
	profile: "debug" | "release" | "unknown";
	mtimeMs: number;
	mtimeIso: string;
	sizeBytes: number;
}

export async function createBenchVm(options: BenchVmOptions = {}): Promise<BenchVm> {
	const commandDirs = [
		resolveBenchCommandsDir(options.commandsDir),
		...(options.wasmCommandDirs ?? []),
	];
	const guestCommandDirs = commandDirs.map(
		(_directory, index) => `/opt/agentos/benchmark-commands/${index}`,
	);
	const commandPaths = new Map<string, string>();
	for (const [index, directory] of commandDirs.entries()) {
		for (const name of readdirSync(directory)) {
			if (!commandPaths.has(name)) {
				commandPaths.set(name, `${guestCommandDirs[index]}/${name}`);
			}
		}
	}
	const sidecar = options.sidecar ?? (await AgentOs.createSidecar());
	const ownsSidecar = options.sidecar === undefined;
	const sidecarPidsBefore = findSidecarProcessIds();
	const runtime = await AgentOs.create({
		defaultSoftware: false,
		...(options.permissions === undefined
			? {}
			: { permissions: options.permissions }),
		...(options.loopbackExemptPorts === undefined
			? {}
			: { loopbackExemptPorts: options.loopbackExemptPorts }),
		mounts: [
			...(options.mounts ?? []).map((mount) => ({
				path: mount.guestPath,
				plugin: {
					id: "host_dir" as const,
					config: { hostPath: mount.hostPath },
				},
				...(mount.readOnly === undefined
					? {}
					: { readOnly: mount.readOnly }),
			})),
			...commandDirs.map((hostPath, index) => ({
				path: guestCommandDirs[index],
				plugin: {
					id: "host_dir" as const,
					config: { hostPath },
				},
				readOnly: true,
			})),
		],
		sidecar: { kind: "explicit", handle: sidecar },
		// Benchmark VM: opt in to the us-resolution guest clock so sub-ms guest
		// samples are real instead of 1ms-floor artifacts.
		highResolutionTime: true,
	});
	const sidecarPidsAfter = findSidecarProcessIds();
	const newSidecarPids = sidecarPidsAfter.filter(
		(pid) => !sidecarPidsBefore.includes(pid),
	);
	if (newSidecarPids.length === 1) {
		benchSidecarPids.set(sidecar, newSidecarPids[0]);
	}
	const processIds = new Set<number>();
	const commandPath = guestCommandDirs.join(":");
	const withCommandPath = (env?: Record<string, string>) => ({
		PATH: `${commandPath}:/opt/agentos/bin:/usr/local/bin:/usr/bin:/bin`,
		...(env ?? {}),
	});

	return {
		writeFile(path, content) {
			return runtime.writeFile(path, content);
		},
		async mkdir(path, options = {}) {
			await runtime.mkdir(path, options);
		},
		async delete(path, options = {}) {
			await runtime.delete(path, options);
		},
		readFile(path) {
			return runtime.readFile(path);
		},
		readDir(path) {
			return runtime.readdir(path);
		},
		readdir(path) {
			return runtime.readdir(path);
		},
		exec(commandLine, execOptions = {}) {
			return runtime.exec(commandLine, {
				...execOptions,
				env: withCommandPath(execOptions.env),
			});
		},
		execArgv(command, args, execOptions = {}) {
			return runtime.execArgv(commandPaths.get(command) ?? command, args, {
				...execOptions,
				env: withCommandPath(execOptions.env),
			});
		},
		async spawnNodeCapture(argsOrProgramPath, env, captureOptions = {}) {
			const args =
				typeof argsOrProgramPath === "string"
					? [argsOrProgramPath]
					: argsOrProgramPath;
			return runtime.execArgv("node", args, {
				env: withCommandPath(env),
				onStdout: captureOptions.onStdout,
				onStderr: captureOptions.onStderr,
			});
		},
		async spawn(command, args, spawnOptions = {}) {
			const proc = await runtime.spawn(commandPaths.get(command) ?? command, args, {
				env: withCommandPath(spawnOptions.env),
				cwd: spawnOptions.cwd,
				onStdout: spawnOptions.onStdout,
				onStderr: spawnOptions.onStderr,
			});
			processIds.add(proc.pid);
			return {
				pid: proc.pid,
				wait: async () => {
					try {
						return await runtime.waitProcess(proc.pid);
					} finally {
						processIds.delete(proc.pid);
					}
				},
			};
		},
		async waitProcess(pid) {
			if (!processIds.has(pid)) {
				throw new Error(`unknown benchmark process pid ${pid}`);
			}
			try {
				return await runtime.waitProcess(pid);
			} finally {
				processIds.delete(pid);
			}
		},
		execWasmCommand(cmd, args, execOptions = {}) {
			return runtime.execArgv(commandPaths.get(cmd) ?? cmd, args, {
				...execOptions,
				env: withCommandPath(execOptions.env),
			});
		},
		async dispose() {
			await runtime.dispose();
			if (ownsSidecar) {
				await sidecar.dispose();
			}
		},
		sidecarPid() {
			return benchSidecarPids.get(sidecar) ?? null;
		},
	};
}

/**
 * Prewarms a benchmark VM before timed sampling:
 * 1. run trivial guest Node code to force isolate/bridge/first-exec setup;
 * 2. for native-baseline WASM lanes, run a one-iteration cpu_loop command;
 * 3. for command ops, run one discarded VM-command sample so that command WASM
 *    compilation is outside the measured sample set.
 */
export async function prewarmBenchVm(
	vm: BenchVm,
	op: BenchmarkOp | CommandBenchmarkOp,
): Promise<void> {
	const nodeResult = await vm.spawnNodeCapture(["-e", ""]);
	if (nodeResult.exitCode !== 0) {
		throw new Error(`guest node prewarm exited ${nodeResult.exitCode}\n${nodeResult.stderr}`);
	}

	if (
		!("runHostCmd" in op) &&
		op.nativeOp &&
		!op.wasmUnsupportedReason &&
		supportsWasmLayer(op.nativeOp) &&
		hasNativeBaselineWasm()
	) {
		const wasmResult = await vm.execWasmCommand(NATIVE_BASELINE_WASM_COMMAND, [
			"--op",
			"cpu_loop",
			"--iters",
			"1",
			"--warmup",
			"0",
			"--base-dir",
			NATIVE_BASELINE_WASM_PREWARM_DIR,
		]);
		if (wasmResult.exitCode !== 0) {
			throw new Error(
				`native-baseline wasm prewarm exited ${wasmResult.exitCode}\n${wasmResult.stderr}`,
			);
		}
	}

	if ("runHostCmd" in op && !op.skipReason) {
		await op.runVmCmd(vm, 1, 0);
	}
}

export function createBenchSidecar(): Promise<AgentOsSidecar> {
	return AgentOs.createSidecar();
}

export function resolveBenchCommandsDir(explicit?: string): string {
	return explicit ?? DEFAULT_COMMANDS_DIR;
}

export function resolveBenchSidecarProvenance(): SidecarBinaryProvenance {
	const path = resolvePublishedSidecarBinary();
	const stat = statSync(path);
	return {
		path,
		profile: inferSidecarProfile(path),
		mtimeMs: stat.mtimeMs,
		mtimeIso: formatPacificIso(stat.mtime),
		sizeBytes: stat.size,
	};
}

export function formatSidecarProvenance(
	provenance: SidecarBinaryProvenance,
): string {
	return `Sidecar binary: ${provenance.path} (${provenance.profile}, mtime ${provenance.mtimeIso}, size ${provenance.sizeBytes} bytes)`;
}

function findSidecarProcessIds(): number[] {
	const processIds: number[] = [];
	for (const entry of readdirSync("/proc")) {
		if (!/^\d+$/.test(entry)) continue;
		try {
			const name = readFileSync(`/proc/${entry}/comm`, "utf8").trim();
			if (name === "agentos-sidecar" || name === "agentos-native-sidecar") {
				processIds.push(Number(entry));
			}
		} catch {
			// The process may exit between the /proc directory and comm reads.
		}
	}
	return processIds.sort((left, right) => left - right);
}

function inferSidecarProfile(path: string): "debug" | "release" | "unknown" {
	if (path.includes("/release/")) return "release";
	if (path.includes("/debug/")) return "debug";
	return "unknown";
}

export function formatPacificIso(date: Date): string {
	const formatter = new Intl.DateTimeFormat("en-CA", {
		timeZone: "America/Los_Angeles",
		year: "numeric",
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		fractionalSecondDigits: 3,
		hourCycle: "h23",
		timeZoneName: "shortOffset",
	});
	const parts = new Map(
		formatter.formatToParts(date).map((part) => [part.type, part.value]),
	);
	return `${parts.get("year")}-${parts.get("month")}-${parts.get("day")}T${parts.get("hour")}:${parts.get("minute")}:${parts.get("second")}.${parts.get("fractionalSecond")}${isoOffset(parts.get("timeZoneName") ?? "GMT")}`;
}

function isoOffset(shortOffset: string): string {
	if (shortOffset === "GMT" || shortOffset === "UTC") return "Z";
	const match = /^GMT([+-])(\d{1,2})(?::(\d{2}))?$/.exec(shortOffset);
	if (!match) return shortOffset;
	const [, sign, hours, minutes = "00"] = match;
	return `${sign}${hours.padStart(2, "0")}:${minutes}`;
}
