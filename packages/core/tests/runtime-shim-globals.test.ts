import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// The per-process runtime shim runs as its own script before the guest's
// entry. Guest code must see the same `require`, `process`, and bindings it
// saw when the shim was part of the entry's source.
describe("runtime shim globals", () => {
	let vm: AgentOs;

	beforeAll(async () => {
		vm = await AgentOs.create();
	}, 30_000);

	afterAll(async () => {
		await vm.dispose();
	});

	async function runNode(args: string[]) {
		let stdout = "";
		let stderr = "";
		const { pid } = await vm.process.spawn("node", args, {
			onStdout: (chunk) => {
				stdout += Buffer.from(chunk).toString("utf8");
			},
			onStderr: (chunk) => {
				stderr += Buffer.from(chunk).toString("utf8");
			},
		});
		const { exitCode } = await vm.process.wait(pid);
		return { exitCode, stdout, stderr };
	}

	test("an ES module entry that declares its own require can require builtins", async () => {
		await vm.mkdir("/tmp/shim-esm", { recursive: true });
		await vm.writeFile("/tmp/shim-esm/package.json", '{"type":"module"}');
		// Minified, as bundlers emit it: no spaces and an aliased createRequire.
		await vm.writeFile(
			"/tmp/shim-esm/cli.js",
			'import{createRequire as H}from"node:module";const require=H(import.meta.url);console.log(typeof require("node:fs").readFileSync)',
		);

		expect(await runNode(["/tmp/shim-esm/cli.js"])).toEqual({
			exitCode: 0,
			stdout: "function\n",
			stderr: "",
		});
	}, 30_000);

	test("Python starts Pyodide, whose bootstrap reads process.binding('constants')", async () => {
		await expect(vm.python.evaluate("6 * 7")).resolves.toMatchObject({
			outcome: "succeeded",
			value: 42,
		});
	}, 60_000);
});
