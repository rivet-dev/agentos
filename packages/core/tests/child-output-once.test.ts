import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const onceSource = readFileSync(new URL("./fixtures/child-output-once.cjs", import.meta.url), "utf8");
const removalSource = readFileSync(new URL("./fixtures/child-output-removal.cjs", import.meta.url), "utf8");
type Channel = "stdout" | "stderr";
type Mode = "throw-once" | "add-once" | "reentrant-once" | "registration-order" | "manual";
type Observation = {
	mode: Mode; channel: Channel; handled: number; first: number; later: number;
	closed: number; ended: number; caught: number; code: number;
	trace: string[]; chunks: string[]; contexts: string[];
};
function program(source: string, key: string, config: Record<string, string>): string {
	return "globalThis." + key + "=" + JSON.stringify(config) + ";\n" + source;
}
function observation(output: string, marker: string): unknown {
	const lines = output.split("\n").filter((line) => line.startsWith(marker));
	expect(lines, output).toHaveLength(1);
	return JSON.parse(lines[0].slice(marker.length));
}
function native(source: string, key: string, config: Record<string, string>, marker: string): unknown {
	const result = spawnSync(process.execPath, ["-e", program(source, key, { ...config, command: process.execPath })], {
		encoding: "utf8", timeout: 5_000, maxBuffer: 128 * 1024,
	});
	if (result.error) throw result.error;
	expect(result.signal, result.stderr).toBeNull();
	expect(result.status, result.stderr).toBe(0);
	return observation(result.stdout, marker);
}
async function guest(source: string, key: string, config: Record<string, string>, marker: string): Promise<unknown> {
	let vm: AgentOs | undefined;
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
	let value: unknown;
	const failures: unknown[] = [];
	try {
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({ defaultSoftware: false, limits: { jsRuntime: { capturedOutputLimitBytes: 128 * 1024 } }, sidecar: { kind: "explicit", handle: sidecar }, permissions: {
			fs: "allow", childProcess: "allow", process: "allow", env: "allow", network: "deny", hostFunction: "deny",
		} });
		const result = await vm.process.execFile("node", ["-e", program(source, key, { ...config, command: "node" })], {
			timeoutMs: 5_000, output: { capture: "all" },
		});
		assert(typeof result.stdout === "string", "stdout capture is missing");
		assert(typeof result.stderr === "string", "stderr capture is missing");
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdoutTruncated).toBe(false);
		expect(result.stderrTruncated).toBe(false);
		value = observation(result.stdout, marker);
		expect((await vm.process.list()).filter((entry) => entry.state === "running")).toEqual([]);
	} catch (error) { failures.push(error); }
	finally {
		if (vm) { try { await vm.dispose(); } catch (error) { failures.push(error); } }
		if (sidecar) { try { await sidecar.dispose(); } catch (error) { failures.push(error); } }
	}
	if (failures.length) throw new AggregateError(failures, "child output observation or owning cleanup failed");
	return value;
}
const traces: Record<Mode, string[]> = {
	"throw-once": ["first:ONE", "handled", "later:TWO"],
	"add-once": ["first:ONE", "later:TWO"],
	"reentrant-once": ["first:ONE", "later:NESTED"],
	"registration-order": ["A:ONE", "B:ONE", "C:ONE", "D:ONE", "B:TWO", "D:TWO"],
	"manual": ["first:MANUAL_ONE", "later:MANUAL_TWO"],
};
describe("child output once registration ownership", () => {
	for (const channel of ["stdout", "stderr"] as const) {
		for (const mode of Object.keys(traces) as Mode[]) {
			test(`${channel}: ${mode} matches native callbacks and exact output`, async () => {
				const expected = native(onceSource, "__onceProbe", { mode, channel }, "ONCE_RESULT=") as Observation;
				expect(expected).toMatchObject({ mode, channel, first: 1, later: 1, closed: 1, ended: 1, code: 0,
					handled: mode === "throw-once" ? 1 : 0, caught: mode === "manual" ? 1 : 0 });
				expect(expected.trace).toEqual(traces[mode]);
				expect(expected.contexts.every((value) => value === "once-child-owner")).toBe(true);
				expect(expected.chunks).toEqual(mode === "manual" ? ["MANUAL_ONE", "MANUAL_TWO"] : mode === "reentrant-once" ? ["ONE", "NESTED", "TWO"] : ["ONE", "TWO"]);
				expect(await guest(onceSource, "__onceProbe", { mode, channel }, "ONCE_RESULT=")).toEqual(expected);
			}, 20_000);
		}
		test(`${channel}: original callbacks remove the latest registration and preserve listener receiver`, async () => {
			const expected = native(removalSource, "__removalProbe", { channel }, "REMOVAL_RESULT=");
			const rows = [
				{ mode: "off-original", trace: [] },
				{ mode: "removeListener-original", trace: [] },
				{ mode: "remove-last-once", trace: ["F:X", "M:X", "M:Y"] },
				{ mode: "remove-last-on", trace: ["F:X", "M:X"] },
				{ mode: "remove-later-once-keeps-on", trace: ["F:X", "F:Y"] },
				{ mode: "remove-later-on-keeps-once", trace: ["F:X"] },
				{ mode: "removal-during-dispatch-keeps-snapshot", trace: ["first:X", "later:X", "first:Y"] },
				{ mode: "duplicate-once-independent", trace: ["F:X", "F:X"] },
			];
			expect(expected).toEqual({ channel, rows, receiverCount: rows.reduce((count, row) => count + row.trace.length, 0), contextPreserved: true, ended: 1, closed: 1, code: 0 });
			expect(await guest(removalSource, "__removalProbe", { channel }, "REMOVAL_RESULT=")).toEqual(expected);
		}, 20_000);
	}
});
