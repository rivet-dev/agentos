import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Both lanes run this exact program against their real child-process/stream
// modules. It needs no third-party package or synthetic bridge implementation.
function program(command: string): string {
	return `
const { spawn } = require('node:child_process');
const { PassThrough, Writable } = require('node:stream');
const immediate = () => new Promise(resolve => setImmediate(resolve));
const settle = async () => { await immediate(); await Promise.resolve(); await immediate(); };
const close = child => new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
const launch = () => spawn(${JSON.stringify(command)}, ['-e', 'process.stdout.write("OUT");process.stderr.write("ERR");'], { stdio: ['ignore', 'pipe', 'pipe'] });
const destination = () => {
  const stream = new PassThrough();
  const state = { bytes: '', end: 0, finish: 0, unpipe: 0 };
  stream.on('data', chunk => { state.bytes += chunk.toString(); });
  stream.on('end', () => state.end++);
  stream.on('finish', () => state.finish++);
  stream.on('unpipe', () => state.unpipe++);
  return { stream, state };
};
(async () => {
  const rows = [];
  for (const channel of ['stdout', 'stderr']) {
    const child = launch(), dest = destination(), source = child[channel];
    let sourceEnd = 0, sourceOnceEnd = 0;
    source.pipe(dest.stream);
    source.on('end', () => sourceEnd++);
    source.once('end', () => sourceOnceEnd++);
    await close(child); await settle();
    rows.push({ mode: 'default', channel, ...dest.state, sourceEnd, sourceOnceEnd });
    dest.stream.destroy();
  }
  {
    const child = launch(), dest = destination();
    child.stdout.pipe(dest.stream, { end: false });
    await close(child); await settle();
    rows.push({ mode: 'end-false-before', ...dest.state });
    dest.stream.end(); await settle();
    rows.push({ mode: 'end-false-after', ...dest.state });
    dest.stream.destroy();
  }
  {
    const child = launch(), dest = destination();
    child.stdout.pipe(dest.stream);
    child.stdout.unpipe(dest.stream);
    child.stdout.on('data', () => {});
    await close(child); await settle();
    rows.push({ mode: 'unpipe', ...dest.state });
    dest.stream.destroy();
  }
  {
    const child = launch(); child.stdout.on('data', () => {});
    await close(child); await settle();
    const dest = destination();
    child.stdout.pipe(dest.stream);
    await settle();
    rows.push({ mode: 'already-ended', ...dest.state });
    dest.stream.destroy();
  }
  for (const channel of ['stdout', 'stderr']) {
    const child = launch(), target = process[channel];
    const originalEnd = target.end; let endCalls = 0;
    target.end = () => { endCalls++; throw new Error('must not end process stdio'); };
    try {
      child[channel].pipe(target);
      await close(child); await settle();
      rows.push({ mode: 'stdio', channel, endCalls });
    } finally { target.end = originalEnd; }
  }
  {
    const child = launch(); let errors = 0, unpipes = 0, sourceBytes = '';
    const dest = new Writable({ write(chunk, encoding, callback) { callback(new Error('synthetic destination error')); } });
    dest.on('error', error => { if (error.message !== 'synthetic destination error') throw error; errors++; });
    dest.on('unpipe', () => unpipes++);
    child.stdout.on('data', chunk => { sourceBytes += chunk.toString(); });
    child.stdout.pipe(dest);
    await close(child); await settle();
    rows.push({ mode: 'destination-error', errors, unpipes, sourceBytes });
    dest.destroy();
  }
  {
    const child = launch(), first = destination(), second = destination(); let sourceEnd = 0;
    child.stdout.pipe(first.stream); child.stdout.pipe(second.stream);
    child.stdout.on('end', () => sourceEnd++);
    await close(child); await settle();
    rows.push({ mode: 'two-destinations', first: first.state, second: second.state, sourceEnd });
    first.stream.destroy(); second.stream.destroy();
  }
  for (const mode of ['both', 'unpipe-one', 'unpipe-all', 'remove-first-noend', 'remove-first-end']) {
    const child = spawn(${JSON.stringify(command)}, ['-e', 'process.stdout.write("X");'], { stdio: ['ignore', 'pipe', 'ignore'] });
    const dest = destination(); let pipes = 0, endCalls = 0;
    dest.stream.on('pipe', () => pipes++);
    const originalEnd = dest.stream.end;
    dest.stream.end = function(...args) { endCalls++; return Reflect.apply(originalEnd, this, args); };
    child.stdout.pipe(dest.stream, { end: mode !== 'remove-first-noend' });
    child.stdout.pipe(dest.stream, { end: mode !== 'remove-first-end' });
    if (mode === 'unpipe-one' || mode.startsWith('remove-first')) child.stdout.unpipe(dest.stream);
    if (mode === 'unpipe-all') child.stdout.unpipe();
    child.stdout.on('data', () => {});
    await close(child); await settle();
    rows.push({ mode: 'duplicate-' + mode, ...dest.state, pipes, endCalls });
    dest.stream.destroy();
  }
  process.stdout.write('\\nPIPE_RESULT=' + JSON.stringify(rows) + '\\n');
})().catch(error => { console.error(error); process.exitCode = 1; });
`;
}

function resultRows(output: string): unknown {
	const line = output.split("\n").find((line) => line.startsWith("PIPE_RESULT="));
	if (!line) throw new Error(`pipe observation missing: ${output}`);
	return JSON.parse(line.slice("PIPE_RESULT=".length));
}

describe("child output pipe lifecycle", () => {
	test("matches native EOF, unpipe, stdio and destination-error ownership", async () => {
		const native = spawnSync(process.execPath, ["-e", program(process.execPath)], {
			encoding: "utf8", timeout: 15_000, maxBuffer: 128 * 1024,
		});
		expect(native.status, native.stderr).toBe(0);
		const expected = resultRows(native.stdout);
		expect(expected).toContainEqual({ mode: "default", channel: "stdout", bytes: "OUT", end: 1, finish: 1, unpipe: 1, sourceEnd: 1, sourceOnceEnd: 1 });
		expect(expected).toContainEqual({ mode: "default", channel: "stderr", bytes: "ERR", end: 1, finish: 1, unpipe: 1, sourceEnd: 1, sourceOnceEnd: 1 });
		expect(expected).toContainEqual({ mode: "end-false-before", bytes: "OUT", end: 0, finish: 0, unpipe: 1 });
		expect(expected).toContainEqual({ mode: "destination-error", errors: 1, unpipes: 1, sourceBytes: "OUT" });
		expect(expected).toContainEqual({ mode: "two-destinations", first: { bytes: "OUT", end: 1, finish: 1, unpipe: 1 }, second: { bytes: "OUT", end: 1, finish: 1, unpipe: 1 }, sourceEnd: 1 });
		expect(expected).toContainEqual({ mode: "duplicate-both", bytes: "XX", end: 1, finish: 1, unpipe: 2, pipes: 2, endCalls: 2 });
		expect(expected).toContainEqual({ mode: "duplicate-unpipe-one", bytes: "X", end: 1, finish: 1, unpipe: 2, pipes: 2, endCalls: 1 });
		expect(expected).toContainEqual({ mode: "duplicate-unpipe-all", bytes: "", end: 0, finish: 0, unpipe: 2, pipes: 2, endCalls: 0 });
		expect(expected).toContainEqual({ mode: "duplicate-remove-first-noend", bytes: "X", end: 1, finish: 1, unpipe: 2, pipes: 2, endCalls: 1 });
		expect(expected).toContainEqual({ mode: "duplicate-remove-first-end", bytes: "X", end: 0, finish: 0, unpipe: 2, pipes: 2, endCalls: 0 });
		let vm: AgentOs | undefined;
		const failures: unknown[] = [];
		try {
			vm = await AgentOs.create({ defaultSoftware: false, limits: { jsRuntime: { capturedOutputLimitBytes: 128 * 1024 } }, permissions: {
				fs: "allow", childProcess: "allow", process: "allow", env: "allow", network: "deny", hostFunction: "deny",
			} });
			const result = await vm.process.execFile("node", ["-e", program("node")], {
				timeoutMs: 15_000, output: { capture: "all" },
			});
			assert(typeof result.stdout === "string", "stdout capture is missing");
			assert(typeof result.stderr === "string", "stderr capture is missing");
			expect(result.exitCode, result.stderr).toBe(0);
			expect(resultRows(result.stdout)).toEqual(expected);
			expect((await vm.process.list()).filter((entry) => entry.state === "running")).toEqual([]);
		} catch (error) { failures.push(error); }
		finally {
			if (vm) { try { await vm.dispose(); } catch (error) { failures.push(error); } }
		}
		if (failures.length) throw new AggregateError(failures, "child pipe test or owning cleanup failed");
	}, 35_000);
});

// Separate callback-owner controls so a handled-error regression cannot prevent
// the independent EOF program above from publishing its observations.
type CallbackMode = "handled" | "unhandled" | "handler-throws" | "manual";
function callbackProgram(command: string, channel: "stdout" | "stderr", mode: CallbackMode): string {
	const childSource = mode === "handled"
		? `process.stdin.once("data", () => { process.${channel}.write("TWO"); process.stdin.pause(); });process.stdin.resume();process.${channel}.write("ONE");`
		: mode === "manual" ? "" : `process.${channel}.write("ONE");`;
	return String.raw`
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { AsyncLocalStorage } = require('node:async_hooks');
const mode = ${JSON.stringify(mode)}, channel = ${JSON.stringify(channel)};
const storage = new AsyncLocalStorage();
let child, handled = 0, calls = 0, onceCalls = 0, onceValue = '', closed = 0, ended = 0, caught = false;
const onUncaught = error => {
  assert.equal(error.message, 'synthetic child callback error');
  assert.equal(storage.getStore(), 'child-owner', 'error handler must run in child async context');
  handled++;
  if (mode === 'handler-throws') {
    process.stdout.write('CALLBACK_HANDLER\n');
    throw new Error('synthetic handler failure');
  }
  child.stdin.end('next');
};
(async () => {
  if (mode !== 'unhandled') process.on('uncaughtException', onUncaught);
  try {
    await storage.run('child-owner', async () => {
      child = spawn(${JSON.stringify(command)}, ['-e', ${JSON.stringify(childSource)}], { stdio: ['pipe', 'pipe', 'pipe'] });
      const output = child[channel];
      child[channel === 'stdout' ? 'stderr' : 'stdout'].on('data', () => {});
      output.on('data', () => {
        assert.equal(storage.getStore(), 'child-owner', 'data callback context');
        if (++calls === 1) throw new Error('synthetic child callback error');
      });
      output.once('data', chunk => { onceCalls++; onceValue += chunk.toString(); });
      output.once('end', () => { ended++; });
      if (mode === 'manual') {
        try { output.emit('data', Buffer.from('MANUAL')); }
        catch (error) { assert.equal(error.message, 'synthetic child callback error'); caught = true; }
        assert(caught, 'public emit must throw synchronously to its caller');
        child.stdin.end();
      }
      const code = await new Promise((resolve, reject) => {
        child.once('error', reject);
        child.once('close', code => { closed++; resolve(code); });
      });
      assert.equal(code, 0);
      await new Promise(resolve => setImmediate(resolve));
    });
  } finally { if (mode !== 'unhandled') process.off('uncaughtException', onUncaught); }
  process.stdout.write('CALLBACK_RESULT=' + JSON.stringify({ mode, channel, handled, calls, onceCalls, onceValue, closed, ended, caught }) + '\n');
})().catch(error => { console.error(error); process.exitCode = 1; });
`;
}

function callbackRows(output: string): unknown {
	const line = output.split("\n").find((line) => line.startsWith("CALLBACK_RESULT="));
	if (!line) throw new Error(`callback observation missing: ${output}`);
	return JSON.parse(line.slice("CALLBACK_RESULT=".length));
}

async function callbackCase(channel: "stdout" | "stderr", mode: CallbackMode): Promise<void> {
	const native = spawnSync(process.execPath, ["-e", callbackProgram(process.execPath, channel, mode)], {
		encoding: "utf8", timeout: 15_000, maxBuffer: 128 * 1024,
	});
	if (native.error) throw native.error;
	expect(native.signal, native.stderr).toBeNull();
	expect(native.status, native.stderr).not.toBeNull();
	const handled = mode === "handled" || mode === "manual";
	let expected: unknown;
	if (handled) {
		expect(native.status, native.stderr).toBe(0);
		expected = callbackRows(native.stdout);
		expect(expected).toEqual({ mode, channel, handled: mode === "handled" ? 1 : 0,
			calls: mode === "handled" ? 2 : 1, onceCalls: mode === "handled" ? 1 : 0,
			onceValue: mode === "handled" ? "TWO" : "", closed: 1, ended: 1, caught: mode === "manual" });
	} else {
		expect(native.status, native.stderr).not.toBe(0);
		expect(native.stdout).not.toContain("CALLBACK_RESULT=");
		expect(native.stderr).toContain(mode === "handler-throws" ? "synthetic handler failure" : "synthetic child callback error");
		expect(native.stdout).toBe(mode === "handler-throws" ? "CALLBACK_HANDLER\n" : "");
	}
	let vm: AgentOs | undefined;
	const failures: unknown[] = [];
	try {
		vm = await AgentOs.create({ defaultSoftware: false, limits: { jsRuntime: { capturedOutputLimitBytes: 128 * 1024 } }, permissions: {
			fs: "allow", childProcess: "allow", process: "allow", env: "allow", network: "deny", hostFunction: "deny",
		} });
		const result = await vm.process.execFile("node", ["-e", callbackProgram("node", channel, mode)], {
			timeoutMs: 15_000, output: { capture: "all" },
		});
		assert(typeof result.stdout === "string", "stdout capture is missing");
		assert(typeof result.stderr === "string", "stderr capture is missing");
		if (handled) {
			expect(result.exitCode, result.stderr).toBe(0);
			expect(callbackRows(result.stdout)).toEqual(expected);
		} else {
			expect(result.exitCode, result.stderr).not.toBe(0);
			expect(result.stdout).not.toContain("CALLBACK_RESULT=");
			expect(result.stdout).toBe(mode === "handler-throws" ? "CALLBACK_HANDLER\n" : "");
			if (mode === "handler-throws") expect(result.stderr).toMatch(/uncaughtException handler failed|synthetic handler failure/);
			else expect(result.stderr).toContain("synthetic child callback error");
		}
		expect((await vm.process.list()).filter((entry) => entry.state === "running")).toEqual([]);
	} catch (error) { failures.push(error); }
	finally { if (vm) { try { await vm.dispose(); } catch (error) { failures.push(error); } } }
	if (failures.length) throw new AggregateError(failures, `child callback owner or cleanup failed: ${channel}/${mode}`);
}

describe("child output callback error ownership", () => {
	for (const channel of ["stdout", "stderr"] as const) {
		test(`handled ${channel} error retains context, later data and normal retirement`, async () => {
			await callbackCase(channel, "handled");
		}, 35_000);
	}
	for (const mode of ["unhandled", "handler-throws", "manual"] as const) {
		test(`preserves ${mode} callback error semantics`, async () => {
			await callbackCase("stdout", mode);
		}, 35_000);
	}
});
