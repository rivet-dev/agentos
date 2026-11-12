import { afterEach, describe, expect, test } from "vitest";
import { AgentOs, createSnapshotExport } from "../src/index.js";

describe("stdio with filesystem permission rules", () => {
	let vm: AgentOs | undefined;
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	for (const structuredPolicy of [false, true]) {
		for (const deniedReads of [0, 6]) {
			test(`preserves parent and child ASCII and NUL output with ${structuredPolicy ? "read rules" : "scalar allow"} after ${deniedReads} read attempts`, async () => {
				vm = await AgentOs.create({
					defaultSoftware: false,
					limits: { resources: { maxProcesses: 4 } },
					permissions: {
						fs: structuredPolicy
							? {
									default: "allow",
									rules: [
										{
											mode: "deny",
											operations: ["read"],
											paths: ["/denied.txt"],
										},
									],
								}
							: "allow",
						childProcess: "allow",
						process: "allow",
					},
					rootFilesystem: {
						lowers: [
							createSnapshotExport([
								{
									path: "/denied.txt",
									type: "file",
									mode: "0644",
									content: "denied",
									encoding: "utf8",
								},
							]),
						],
					},
				});
				const stdout: number[] = [];
				const stderr: number[] = [];
				const capture = (target: number[], chunk: Uint8Array) => {
					if (target.length + chunk.byteLength > 4096) {
						throw new Error("stdio regression output exceeded 4096 bytes");
					}
					target.push(...chunk);
				};
				const childCode =
					"process.stdout.write(Buffer.from([0,65,66,67]));process.stderr.write(Buffer.from([68,0,69,70]));";
				const source = `
const cp = require('child_process');
for (let i = 0; i < ${deniedReads}; i++) {
  const result = cp.spawnSync('node', ['-e', "require('fs').readFileSync('/denied.txt')"]);
  if (${structuredPolicy} && result.status === 0) throw new Error('read denial was bypassed');
}
console.log('parent');
const child = cp.spawnSync('node', ['-e', ${JSON.stringify(childCode)}]);
if (child.status !== 0) throw new Error(String(child.stderr));
process.stdout.write(child.stdout);
process.stderr.write(child.stderr);
`;
				const { pid } = await vm.process.spawn("node", ["-e", source], {
					timeoutMs: 10_000,
					onStdout: (chunk) => capture(stdout, chunk),
					onStderr: (chunk) => capture(stderr, chunk),
				});
				const { exitCode } = await vm.process.wait(pid);
				expect(exitCode, new TextDecoder().decode(new Uint8Array(stderr))).toBe(
					0,
				);
				expect(stdout).toEqual([
					...new TextEncoder().encode("parent\n"),
					0,
					65,
					66,
					67,
				]);
				expect(stderr).toEqual([68, 0, 69, 70]);
			}, 15_000);
		}
	}
});
