import { spawnSync } from "node:child_process";
import { StringDecoder } from "node:string_decoder";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { forkIpcOwnershipProbe } from "./fixtures/fork-ipc-ownership-probes.js";

for (const mode of ["direct", "disconnect", "nested", "context"] as const)
	test(`fork IPC preserves ${mode} descriptor ownership`, async () => {
		const source = forkIpcOwnershipProbe(mode);
		const host = spawnSync(
			process.execPath,
			["--input-type=module", "-e", source],
			{ encoding: "utf8", timeout: 10000, maxBuffer: 65536 },
		);
		expect(host.status, host.stderr).toBe(0);
		const expected = JSON.parse(host.stdout);
		expect(expected.timedOut).toBe(false);
		expect(expected.errors).toEqual([]);
		expect(expected.exit).toEqual({ code: 0, signal: null });
		if (mode === "context")
			expect(expected.contexts).toEqual({
				message: ["creator", "creator"],
				send: ["sender"],
				disconnect: ["creator"],
			});
		const sidecar = await AgentOs.createSidecar();
		let vm: AgentOs | undefined;
		const failures: unknown[] = [];
		try {
			vm = await AgentOs.create({
				sidecar: { kind: "explicit", handle: sidecar },
				defaultSoftware: false,
				permissions: { network: "deny", childProcess: "allow" },
			});
			let stdout = "",
				stderr = "",
				captureError: Error | undefined;
			const decoders = {
				stdout: new StringDecoder("utf8"),
				stderr: new StringDecoder("utf8"),
			};
			const capturedBytes = { stdout: 0, stderr: 0 };
			const capture = (target: "stdout" | "stderr", chunk: Uint8Array) => {
				if (captureError) return;
				if (capturedBytes[target] + chunk.byteLength > 65536) {
					captureError = new Error("IPC ownership output exceeded 64KiB");
					return;
				}
				capturedBytes[target] += chunk.byteLength;
				const text = decoders[target].write(Buffer.from(chunk));
				if (target === "stdout") stdout += text;
				else stderr += text;
			};
			const proc = await vm.process.spawn(
				"node",
				["--input-type=module", "-e", source],
				{
					timeoutMs: 10000,
					onStdout: (chunk) => capture("stdout", chunk),
					onStderr: (chunk) => capture("stderr", chunk),
				},
			);
			const exit = await vm.process.wait(proc.pid);
			stdout += decoders.stdout.end();
			stderr += decoders.stderr.end();
			console.log(
				JSON.stringify({
					kind: "fork-ipc-ownership",
					mode,
					node: expected,
					guest: { stdout, stderr, exit },
				}),
			);
			if (captureError) throw captureError;
			expect(exit.exitCode, stderr).toBe(0);
			expect(JSON.parse(stdout)).toEqual(expected);
		} catch (error) {
			failures.push(error);
		} finally {
			for (const cleanup of [() => vm?.dispose(), () => sidecar.dispose()])
				try {
					await cleanup();
				} catch (error) {
					failures.push(error);
				}
		}
		if (failures.length)
			throw new AggregateError(
				failures,
				failures
					.map((error) =>
						String(
							error instanceof Error ? (error.stack ?? error.message) : error,
						),
					)
					.join("\n\n"),
			);
	}, 20000);
