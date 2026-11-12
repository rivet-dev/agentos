import { spawnSync } from "node:child_process";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

function binaryCaptureProgram(command: string): string {
	const child =
		"process.stdout.write(Buffer.from([0,255,128,65,226,130,172]));process.stderr.write(Buffer.from([254,0,127,66]));";
	return `
const cp = require('child_process');
const rows = [];
for (const encoding of [null, 'buffer', 'utf8', 'hex', 'base64', 'latin1']) {
  const result = cp.spawnSync(${JSON.stringify(command)}, ['-e', ${JSON.stringify(child)}], {
    ...(encoding === null ? {} : { encoding }), timeout: 2000, maxBuffer: 1024,
  });
  if (result.error) throw result.error;
  const capture = value => Buffer.isBuffer(value) ? { buffer: [...value] } : { text: value };
  rows.push({ encoding, status: result.status, signal: result.signal,
    stdout: capture(result.stdout), stderr: capture(result.stderr),
    outputIdentity: result.output[1] === result.stdout && result.output[2] === result.stderr });
}
console.log(JSON.stringify(rows));
`;
}

describe("synchronous child binary output", () => {
	let vm: AgentOs | undefined;
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	for (const structuredPolicy of [false, true]) {
		test(`matches native byte capture and requested encodings with ${structuredPolicy ? "read rules" : "scalar allow"}`, async () => {
			const native = spawnSync(
				process.execPath,
				["-e", binaryCaptureProgram(process.execPath)],
				{
					encoding: "utf8",
					timeout: 10_000,
					maxBuffer: 32 * 1024,
				},
			);
			expect(native.status, native.stderr).toBe(0);
			const expected = JSON.parse(native.stdout);
			expect(expected[0].stdout.buffer).toEqual([
				0, 255, 128, 65, 226, 130, 172,
			]);
			expect(expected[0].stderr.buffer).toEqual([254, 0, 127, 66]);
			vm = await AgentOs.create({
				defaultSoftware: false,
				permissions: {
					fs: structuredPolicy
						? {
								default: "allow",
								rules: [
									{
										mode: "deny",
										operations: ["read"],
										paths: ["/unrelated-denied-input"],
									},
								],
							}
						: "allow",
					childProcess: "allow",
					process: "allow",
				},
			});
			const result = await vm.process.execFile(
				"node",
				["-e", binaryCaptureProgram("node")],
				{
					timeoutMs: 10_000,
					output: { capture: "all", maxBufferedBytes: 32 * 1024 },
				},
			);
			expect(result.exitCode, result.stderr).toBe(0);
			expect(JSON.parse(result.stdout)).toEqual(expected);
		}, 20_000);
	}
});
