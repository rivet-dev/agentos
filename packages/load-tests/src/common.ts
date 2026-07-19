import {
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	readlinkSync,
	writeFileSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";

export interface ProcessTreeSample {
	timestampMs: number;
	rssBytes: number;
	/** Proportional set size across the tree (shared pages divided by sharers).
	 * More leak-accurate than RSS; -1 when smaps_rollup is unavailable. */
	pssBytes: number;
	pidCount: number;
	threadCount: number;
	fdCount: number;
	/** Controller Node heap used (process.memoryUsage().heapUsed). */
	hostHeapUsedBytes: number;
	pids: number[];
}

export interface TimedProbe {
	startedAtMs: number;
	durationMs: number;
	ok: boolean;
	error?: string;
}

export const artifactRoot = resolve(
	process.env.LOAD_TEST_ARTIFACT_DIR ?? ".artifacts/load-tests",
);

export function integerEnv(name: string, fallback: number): number {
	const raw = process.env[name];
	if (raw === undefined) return fallback;
	const value = Number.parseInt(raw, 10);
	if (!Number.isSafeInteger(value) || value < 1) {
		throw new Error(`${name} must be a positive integer, received ${raw}`);
	}
	return value;
}

export function numberEnv(name: string, fallback: number): number {
	const raw = process.env[name];
	if (raw === undefined) return fallback;
	const value = Number(raw);
	if (!Number.isFinite(value) || value < 0) {
		throw new Error(`${name} must be a non-negative number, received ${raw}`);
	}
	return value;
}

export function csvIntegersEnv(name: string, fallback: number[]): number[] {
	const raw = process.env[name];
	if (raw === undefined) return fallback;
	const values = raw.split(",").map((part) => Number.parseInt(part.trim(), 10));
	if (
		values.length === 0 ||
		values.some((value) => !Number.isSafeInteger(value) || value < 1)
	) {
		throw new Error(`${name} must be comma-separated positive integers`);
	}
	return values;
}

export function sleep(ms: number): Promise<void> {
	return new Promise((resolveSleep) => setTimeout(resolveSleep, ms));
}

export async function withTimeout<T>(
	label: string,
	promise: Promise<T>,
	timeoutMs: number,
): Promise<T> {
	let timer: NodeJS.Timeout | undefined;
	try {
		return await Promise.race([
			promise,
			new Promise<T>((_resolve, reject) => {
				timer = setTimeout(
					() => reject(new Error(`${label} timed out after ${timeoutMs}ms`)),
					timeoutMs,
				);
			}),
		]);
	} finally {
		if (timer) clearTimeout(timer);
	}
}

function readNumber(path: string): number {
	try {
		return Number.parseInt(readFileSync(path, "utf8").trim(), 10) || 0;
	} catch {
		return 0;
	}
}

function childPids(pid: number): number[] {
	try {
		const text = readFileSync(`/proc/${pid}/task/${pid}/children`, "utf8").trim();
		return text
			? text
					.split(/\s+/)
					.map(Number)
					.filter((value) => Number.isSafeInteger(value) && value > 0)
			: [];
	} catch {
		return [];
	}
}

function pidRssBytes(pid: number): number {
	try {
		const pages = Number.parseInt(
			readFileSync(`/proc/${pid}/statm`, "utf8").split(/\s+/)[1] ?? "0",
			10,
		);
		return (Number.isFinite(pages) ? pages : 0) * 4096;
	} catch {
		return 0;
	}
}

/** Pss in bytes from smaps_rollup, or -1 if unavailable for this pid. */
function pidPssBytes(pid: number): number {
	try {
		const rollup = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8");
		const match = rollup.match(/^Pss:\s+(\d+)\s+kB/m);
		return match ? Number(match[1]) * 1024 : -1;
	} catch {
		return -1;
	}
}

export function sampleProcessTree(rootPid = process.pid): ProcessTreeSample {
	const queue = [rootPid];
	const visited = new Set<number>();
	let rssBytes = 0;
	let pssBytes = 0;
	let pssAvailable = false;
	let threadCount = 0;
	let fdCount = 0;

	while (queue.length > 0) {
		const pid = queue.pop();
		if (pid === undefined || visited.has(pid) || !existsSync(`/proc/${pid}`)) {
			continue;
		}
		visited.add(pid);
		rssBytes += pidRssBytes(pid);
		const pss = pidPssBytes(pid);
		if (pss >= 0) {
			pssBytes += pss;
			pssAvailable = true;
		}
		try {
			threadCount += readdirSync(`/proc/${pid}/task`).length;
		} catch {
			// The process may exit between census operations.
		}
		try {
			fdCount += readdirSync(`/proc/${pid}/fd`).length;
		} catch {
			// The process may exit between census operations.
		}
		queue.push(...childPids(pid));
	}

	return {
		timestampMs: Date.now(),
		rssBytes,
		pssBytes: pssAvailable ? pssBytes : -1,
		pidCount: visited.size,
		threadCount,
		fdCount,
		hostHeapUsedBytes: process.memoryUsage().heapUsed,
		pids: [...visited].sort((left, right) => left - right),
	};
}

export function cgroupSnapshot(): Record<string, number | string> {
	const root = "/sys/fs/cgroup";
	const fields = [
		"memory.current",
		"memory.peak",
		"memory.max",
		"memory.swap.max",
		"pids.current",
		"pids.max",
	] as const;
	const snapshot: Record<string, number | string> = {};
	for (const field of fields) {
		try {
			const value = readFileSync(join(root, field), "utf8").trim();
			snapshot[field] = /^\d+$/.test(value) ? Number(value) : value;
		} catch {
			// cgroup v1 and non-Linux environments omit these files.
		}
	}
	try {
		for (const line of readFileSync(join(root, "memory.events"), "utf8").trim().split("\n")) {
			const [key, value] = line.split(/\s+/);
			if (key && value) snapshot[`memory.events.${key}`] = Number(value);
		}
	} catch {
		// Optional diagnostic.
	}
	return snapshot;
}

export function linearSlope(values: number[]): number {
	if (values.length < 2) return 0;
	const meanX = (values.length - 1) / 2;
	const meanY = values.reduce((sum, value) => sum + value, 0) / values.length;
	let numerator = 0;
	let denominator = 0;
	for (let index = 0; index < values.length; index += 1) {
		const deltaX = index - meanX;
		numerator += deltaX * (values[index]! - meanY);
		denominator += deltaX * deltaX;
	}
	return denominator === 0 ? 0 : numerator / denominator;
}

export function percentile(values: number[], quantile: number): number {
	if (values.length === 0) return 0;
	const sorted = [...values].sort((left, right) => left - right);
	const index = Math.min(
		sorted.length - 1,
		Math.max(0, Math.ceil(sorted.length * quantile) - 1),
	);
	return sorted[index]!;
}

export function writeArtifact(
	lane: string,
	runId: string,
	value: unknown,
): string {
	const directory = join(artifactRoot, lane);
	mkdirSync(directory, { recursive: true });
	const path = join(directory, `${runId}.json`);
	writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
	return path;
}

export function newRunId(lane: string): string {
	return `${new Date().toISOString().replace(/[:.]/g, "-")}-${lane}-${process.pid}`;
}

export function errorText(error: unknown): string {
	return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

export function runtimeProvenance(): Record<string, unknown> {
	const sidecar = process.env.AGENTOS_SIDECAR_BIN;
	let sidecarTarget: string | undefined;
	try {
		sidecarTarget = sidecar ? readlinkSync(sidecar) : undefined;
	} catch {
		sidecarTarget = sidecar ? basename(sidecar) : undefined;
	}
	return {
		node: process.version,
		platform: process.platform,
		arch: process.arch,
		sidecar,
		sidecarTarget,
		cgroup: cgroupSnapshot(),
	};
}

export function forceGc(): void {
	(globalThis as { gc?: () => void }).gc?.();
}

export function readCgroupNumber(field: string): number {
	return readNumber(join("/sys/fs/cgroup", field));
}
