import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

describe("software projection on the sidecar path", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	test("keeps projected package roots read-only on the sidecar path", async () => {
		vm = await AgentOs.create();

		let stdout = "";
		let stderr = "";
		const { pid } = vm.spawn(
			"node",
			[
				"-e",
				[
					"const fs = require('node:fs');",
					"try {",
					"  fs.appendFileSync('/opt/agentos/bin/cat', '\\nblocked');",
					"  console.log('write:unexpected-success');",
					"} catch (error) {",
					"  console.log('writeError', error && error.code);",
					"}",
				].join(" "),
			],
			{
				onStdout: (chunk) => {
					stdout += Buffer.from(chunk).toString("utf8");
				},
				onStderr: (chunk) => {
					stderr += Buffer.from(chunk).toString("utf8");
				},
			},
		);

		const exitCode = (await vm.process.wait(pid)).exitCode;
		expect({ exitCode, stderr }).toEqual({ exitCode: 0, stderr: "" });
		expect(stdout).not.toContain("write:unexpected-success");
		expect(stdout).toMatch(/writeError (ERR_ACCESS_DENIED|EACCES|EPERM|EROFS)/);
	});

	test("projects the vendored default software bundle", async () => {
		vm = await AgentOs.create();

		expect(await vm.exists("/bin/cat")).toBe(true);
		expect(await vm.exists("/bin/grep")).toBe(true);
	});
});
