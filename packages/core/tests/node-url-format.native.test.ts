import { spawnSync } from "node:child_process";
import { TextDecoder } from "node:util";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { urlFormatProbe } from "./fixtures/node-url-format-probe.js";

test("url.format preserves WHATWG credentials, options and legacy inputs", async () => {
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
	let vm: AgentOs | undefined;
	const failures: unknown[] = [];
	try {
		const argv = ["--input-type=module", "-e", urlFormatProbe];
		const host = spawnSync(process.execPath, argv, {
			encoding: "utf8",
			timeout: 8_000,
			maxBuffer: 262144,
		});
		expect(host.status, host.stderr).toBe(0);
		const expected = JSON.parse(host.stdout);
		expect(expected.rows).toHaveLength(144);
		expect(expected.sameExport).toBe(true);
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		const stdoutDecoder = new TextDecoder(),
			stderrDecoder = new TextDecoder();
		let stdout = "",
			stderr = "",
			bytes = 0,
			overflow = false;
		const capture = (stream: "stdout" | "stderr", chunk: Uint8Array) => {
			bytes += chunk.byteLength;
			if (bytes > 262144) {
				overflow = true;
				return;
			}
			if (stream === "stdout")
				stdout += stdoutDecoder.decode(chunk, { stream: true });
			else stderr += stderrDecoder.decode(chunk, { stream: true });
		};
		const child = await vm.process.spawn("node", argv, {
			timeoutMs: 10_000,
			onStdout: (chunk) => capture("stdout", chunk),
			onStderr: (chunk) => capture("stderr", chunk),
		});
		const exit = await vm.process.wait(child.pid);
		stdout += stdoutDecoder.decode();
		stderr += stderrDecoder.decode();
		expect(overflow, "URL differential output exceeded 256KiB").toBe(false);
		expect(exit.exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(expected);
	} catch (error) {
		failures.push(error);
	} finally {
		for (const cleanup of [() => vm?.dispose(), () => sidecar?.dispose()]) {
			try {
				await cleanup();
			} catch (error) {
				failures.push(error);
			}
		}
	}
	if (failures.length)
		throw new AggregateError(failures, failures.map(String).join("\n"));
}, 30_000);
