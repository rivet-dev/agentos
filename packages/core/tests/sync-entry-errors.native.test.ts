import { type SpawnSyncReturns, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { syncEntryCases } from "./fixtures/sync-entry-errors.js";

it.each(
	syncEntryCases,
)("$name matches Node and leaves the next process clean", async (row) => {
	const directory = mkdtempSync(join(tmpdir(), "agentos-entry-errors-"));
	let reference: SpawnSyncReturns<string>;
	try {
		const entry = join(directory, "entry.mjs");
		if (row.module) writeFileSync(entry, row.source);
		reference = spawnSync(
			process.execPath,
			row.module ? [entry] : ["-e", row.source],
			{
				encoding: "utf8",
				timeout: 3_000,
				maxBuffer: 16_384,
			},
		);
	} finally {
		rmSync(directory, { recursive: true, force: true });
	}
	expect(reference.error).toBeUndefined();
	expect(reference.signal).toBeNull();
	expect(reference.status, reference.stderr).toBe(row.exitCode);
	expect(reference.stdout).toBe(row.stdout);
	if (row.errorText) expect(reference.stderr).toContain(row.errorText);
	else expect(reference.stderr).toBe("");

	const sidecar = await AgentOs.createSidecar();
	let vm: Awaited<ReturnType<typeof AgentOs.create>> | undefined;
	const failures: unknown[] = [];
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: {
				fs: "allow",
				process: "allow",
				childProcess: "allow",
				network: "deny",
			},
		});
		const entry = "/tmp/sync-entry.mjs";
		if (row.module) await vm.filesystem.writeFile(entry, row.source);
		const actual = await vm.process.execFile(
			"node",
			row.module ? [entry] : ["-e", row.source],
			{
				timeoutMs: 3_000,
				output: { capture: "all" },
			},
		);
		// Run before assertions so even a wrong terminal result exercises reuse.
		const next = await vm.process.execFile(
			"node",
			["-e", "queueMicrotask(() => console.log('CLEAN'))"],
			{
				timeoutMs: 3_000,
				output: { capture: "all" },
			},
		);
		console.info(
			"ENTRY_ERROR_CASE",
			JSON.stringify({
				name: row.name,
				actual: {
					exitCode: actual.exitCode,
					stdout: actual.stdout,
					stderr: actual.stderr,
				},
				next: {
					exitCode: next.exitCode,
					stdout: next.stdout,
					stderr: next.stderr,
				},
			}),
		);
		expect(next.exitCode, next.stderr).toBe(0);
		expect(next.stdout).toBe("CLEAN\n");
		expect(next.stderr).toBe("");
		expect(actual.exitCode, actual.stderr).toBe(reference.status);
		expect(actual.stdout).toBe(reference.stdout);
		expect(Buffer.byteLength(actual.stderr ?? "")).toBeLessThanOrEqual(16_384);
		if (row.errorText) expect(actual.stderr).toContain(row.errorText);
		else expect(actual.stderr).toBe("");
	} catch (error) {
		failures.push(error);
	} finally {
		if (vm) {
			try {
				await vm.dispose();
			} catch (error) {
				failures.push(error);
			}
		}
		try {
			await sidecar.dispose();
		} catch (error) {
			failures.push(error);
		}
	}
	if (failures.length) throw new AggregateError(failures, row.name);
}, 15_000);
