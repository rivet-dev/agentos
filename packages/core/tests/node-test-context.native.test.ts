import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import {
	callbackAfterDoneThrowProbe,
	callbackAfterProbe,
} from "./fixtures/node-api-review-probes.js";

const fixture = (failure: boolean | "both") => `
import test,{afterEach} from 'node:test';import assert from 'node:assert/strict';
const seen=[];afterEach(()=>seen.push('global'));
test('first',async t=>{t.after(async()=>{await Promise.resolve();seen.push('local');${failure ? "throw new Error('fixture cleanup failure');" : ""}});seen.push('body');${failure === "both" ? "throw new Error('fixture body failure');" : ""}});
test('second',()=>assert.deepEqual(seen,['body','global','local']));
`;

for (const failure of [false, true, "both"] as const)
	test(`TestContext.after awaits cleanup and ${failure === "both" ? "retains body and cleanup diagnostics" : failure ? "reports cleanup failures" : "runs after global afterEach"}`, async () => {
		const directory = await mkdtemp(join(tmpdir(), "agentos-test-context-"));
		const file = join(directory, "hooks.test.mjs");
		await writeFile(file, fixture(failure));
		const host = spawnSync(
			process.execPath,
			["--test", "--test-reporter=tap", file],
			{ encoding: "utf8", timeout: 10_000 },
		);
		const vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		try {
			await vm.writeFile("/tmp/hooks.test.mjs", fixture(failure));
			let stdout = "",
				stderr = "";
			const proc = await vm.process.spawn(
				"node",
				["--test", "/tmp/hooks.test.mjs"],
				{
					timeoutMs: 10_000,
					onStdout: (c) => {
						stdout += Buffer.from(c).toString();
					},
					onStderr: (c) => {
						stderr += Buffer.from(c).toString();
					},
				},
			);
			expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(
				host.status,
			);
			expect(stdout).toMatch(/^ok 2 - second$/m);
			expect(stdout).toMatch(
				failure ? /^not ok 1 - first$/m : /^ok 1 - first$/m,
			);
			if (failure) expect(stdout).toContain("fixture cleanup failure");
			if (failure === "both") expect(stdout).toContain("fixture body failure");
		} finally {
			try {
				await vm.dispose();
			} finally {
				await rm(directory, { recursive: true, force: true });
			}
		}
	}, 20_000);

test("TestContext.after supports owned callback completion", async () => {
	const directory = await mkdtemp(join(tmpdir(), "agentos-callback-after-"));
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
	let vm: AgentOs | undefined;
	const failures: unknown[] = [];
	try {
		const file = join(directory, "callback.test.mjs");
		// The unchanged finite source packet is supplemented with an asynchronous
		// callback so merely supplying a dummy done function cannot pass.
		const source = callbackAfterProbe.replace(
			"seen.push('after');\n    done();",
			"setTimeout(()=>{seen.push('after');done();},5);",
		);
		expect(source).not.toBe(callbackAfterProbe);
		await writeFile(file, source);
		const host = spawnSync(
			process.execPath,
			["--test", "--test-reporter=tap", file],
			{ encoding: "utf8", timeout: 10000, maxBuffer: 65536 },
		);
		expect(host.status, host.stderr).toBe(0);
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		await vm.writeFile("/tmp/callback.test.mjs", source);
		let stdout = "",
			stderr = "";
		const proc = await vm.process.spawn(
			"node",
			["--test", "/tmp/callback.test.mjs"],
			{
				timeoutMs: 10000,
				onStdout: (c) => {
					stdout += Buffer.from(c).toString();
				},
				onStderr: (c) => {
					stderr += Buffer.from(c).toString();
				},
			},
		);
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		expect(stdout).toMatch(/^ok 1 - owner$/m);
		expect(stdout).toMatch(/^ok 2 - after consumer$/m);
	} catch (error) {
		failures.push(error);
	} finally {
		for (const cleanup of [
			() => vm?.dispose(),
			() => sidecar?.dispose(),
			() => rm(directory, { recursive: true, force: true }),
		])
			try {
				await cleanup();
			} catch (error) {
				failures.push(error);
			}
	}
	if (failures.length)
		throw new AggregateError(failures, failures.map(String).join("\n"));
}, 20000);

test("TestContext.after retains a synchronous throw after done", async () => {
	const directory = await mkdtemp(join(tmpdir(), "agentos-after-done-"));
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
	let vm: AgentOs | undefined;
	const failures: unknown[] = [];
	try {
		const file = join(directory, "after-done.test.mjs");
		await writeFile(file, callbackAfterDoneThrowProbe);
		const host = spawnSync(
			process.execPath,
			["--test", "--test-reporter=tap", file],
			{ encoding: "utf8", timeout: 10000, maxBuffer: 65536 },
		);
		expect(host.status, host.stderr).toBe(1);
		expect(host.stdout).toContain("after-done sentinel");
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		await vm.writeFile("/tmp/after-done.test.mjs", callbackAfterDoneThrowProbe);
		let stdout = "",
			stderr = "",
			captureError: Error | undefined;
		const capture = (target: "stdout" | "stderr", chunk: Uint8Array) => {
			if (captureError) return;
			const value = target === "stdout" ? stdout : stderr;
			if (Buffer.byteLength(value) + chunk.byteLength > 65536) {
				captureError = new Error("callback-after output exceeded 64KiB");
				return;
			}
			if (target === "stdout") stdout += Buffer.from(chunk).toString();
			else stderr += Buffer.from(chunk).toString();
		};
		const proc = await vm.process.spawn(
			"node",
			["--test", "/tmp/after-done.test.mjs"],
			{
				timeoutMs: 10000,
				onStdout: (c) => capture("stdout", c),
				onStderr: (c) => capture("stderr", c),
			},
		);
		const exit = await vm.process.wait(proc.pid);
		if (captureError) throw captureError;
		expect(exit.exitCode, stderr).toBe(1);
		expect(stdout).toMatch(/^not ok 1 - after-done owner$/m);
		expect(stdout).toContain("after-done sentinel");
		expect(stdout).toMatch(/^ok 2 - after-done consumer$/m);
	} catch (error) {
		failures.push(error);
	} finally {
		for (const cleanup of [
			() => vm?.dispose(),
			() => sidecar?.dispose(),
			() => rm(directory, { recursive: true, force: true }),
		])
			try {
				await cleanup();
			} catch (error) {
				failures.push(error);
			}
	}
	if (failures.length)
		throw new AggregateError(failures, failures.map(String).join("\n"));
}, 20000);
