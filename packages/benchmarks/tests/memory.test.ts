import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";
import { retainedMemoryMedian } from "../src/lib/backend-memory.js";
import { readProcessMemorySnapshot, readProcessTreeMemorySnapshot } from "../src/lib/memory.js";

const snapshot = (rssBytes: number, pssBytes = rssBytes) => ({
	rssBytes, pssBytes, peakRssBytes: rssBytes, virtualBytes: rssBytes,
	minorFaults: 0, majorFaults: 0,
});

test("retained memory includes allocations already present at the VM baseline", () => {
	const samples = [
		{ backend: "v8", retained: snapshot(100, 80), retainedDelta: snapshot(-20, -10) },
		{ backend: "v8", retained: snapshot(120, 100), retainedDelta: snapshot(0, 10) },
		{ backend: "wasmtime", retained: snapshot(500), retainedDelta: snapshot(1) },
	];
	assert.equal(retainedMemoryMedian(samples, "v8", "rssBytes"), 110);
	assert.equal(retainedMemoryMedian(samples, "v8", "pssBytes"), 90);
});

test("process-tree memory includes a live worker omitted by PID-only sampling", { skip: process.platform !== "linux" }, async () => {
	const worker = spawn(process.execPath, ["-e", "process.stdout.write('ready'); process.stdin.resume()"], { stdio: ["pipe", "pipe", "inherit"] });
	try {
		await once(worker.stdout!, "data");
		const own = readProcessMemorySnapshot(process.pid);
		const child = readProcessMemorySnapshot(worker.pid!);
		const tree = readProcessTreeMemorySnapshot(process.pid);
		assert(tree.pids.includes(worker.pid!));
		assert(tree.processCount >= 2);
		assert(tree.pssBytes <= tree.rssBytes);
		assert(tree.rssBytes >= own.rssBytes + child.rssBytes - 4 * 1024 * 1024);
	} finally {
		const exited = once(worker, "exit");
		worker.kill();
		await exited;
	}
});

// VmRSS can lag the page walk even when the target has stopped allocating.
const procFixture = (rollupAvailable = true) => (path: string): string => {
	if (path.endsWith("/status")) return "VmRSS: 100 kB\nVmHWM: 150 kB\nVmSize: 1000 kB\n";
	if (path.endsWith("/stat")) return "42 (fixture process) S 0 0 0 0 0 0 7 0 2";
	if (path.endsWith("/smaps_rollup") && rollupAvailable) return "Rss: 140 kB\nPss: 120 kB\n";
	throw new Error(`unavailable proc file: ${path}`);
};

test("RSS and PSS use the same rollup despite stale status resident counters", () => {
	const memory = readProcessMemorySnapshot(42, procFixture());
	assert.equal(memory.rssBytes, 140 * 1024);
	assert.equal(memory.pssBytes, 120 * 1024);
	assert(memory.pssBytes <= memory.rssBytes);
	assert.equal(memory.peakRssBytes, 150 * 1024);
	assert.equal(memory.virtualBytes, 1000 * 1024);
	assert.equal(memory.minorFaults, 7);
	assert.equal(memory.majorFaults, 2);
});

test("unavailable rollup retains status RSS and the unavailable PSS sentinel", () => {
	const memory = readProcessMemorySnapshot(42, procFixture(false));
	assert.equal(memory.rssBytes, 100 * 1024);
	assert.equal(memory.pssBytes, 0);
});
