import { spawnSync } from "node:child_process";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("Node CLI input compatibility", () => {
	let vm: AgentOs;

	beforeAll(async () => {
		vm = await AgentOs.create({ permissions: { network: "deny" } });
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
	});

	test.each([
		{ name: "implicit stdin", args: [], source: "console.log(6 * 7)" },
		{ name: "explicit stdin", args: ["-"], source: "console.log(6 * 7)" },
		{
			name: "module stdin",
			args: ["--input-type=module"],
			source: "console.log(await Promise.resolve(42))",
		},
		{
			name: "module stdin with arguments",
			args: ["--input-type", "module", "-", "first", "--second"],
			source: "console.log(JSON.stringify(process.argv.slice(1)))",
		},
		{ name: "empty stdin", args: [], source: "" },
	])("$name matches native Node", async ({ args, source }) => {
		const native = spawnSync(process.execPath, args, {
			input: source,
			encoding: "utf8",
			timeout: 10_000,
			env: { PATH: process.env.PATH },
		});
		expect(native.error).toBeUndefined();
		expect(native.status, native.stderr).toBe(0);
		const guest = await vm.process.execFile("node", args, {
			stdin: source,
			timeoutMs: 10_000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, JSON.stringify(guest)).toBe(native.status);
		expect(guest.stdout).toBe(native.stdout);
		expect(guest.stderr).toBe(native.stderr);
	});

	test("relative imports from inline code use the execution cwd", async () => {
		await vm.mkdir("/tmp/inline-origin", { recursive: true });
		await vm.writeFile("/tmp/inline-origin/value.mjs", "export default 42;\n");
		const result = await vm.javascript.execute(
			"console.log((await import('./value.mjs')).default)",
			{
				cwd: "/tmp/inline-origin",
				timeoutMs: 10_000,
				output: { capture: "all" },
			},
		);
		expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
		expect(result.stdout).toBe("42\n");
	});

	test("stdin ES modules support static imports and import.meta from cwd", async () => {
		await vm.mkdir("/tmp/stdin-module", { recursive: true });
		await vm.writeFile("/tmp/stdin-module/value.mjs", "export default 42;\n");
		const source = [
			"import value from './value.mjs';",
			"console.log(value, new URL('.', import.meta.url).pathname);",
		].join("\n");
		const result = await vm.process.execFile(
			"node",
			["--input-type=module", "-"],
			{
				cwd: "/tmp/stdin-module",
				stdin: source,
				timeoutMs: 10_000,
				output: { capture: "all" },
			},
		);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("42 /tmp/stdin-module/\n");
		expect(result.stderr).toBe("");
	});

	test("shell pipelines deliver the program to child Node", async () => {
		const result = await vm.process.exec(
			"printf 'console.log(42)\\n' | node --input-type=module",
			{ timeoutMs: 10_000, output: { capture: "all" } },
		);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("42\n");
		expect(result.stderr).toBe("");
	});

	test("valid and invalid leading Node options have distinct outcomes", async () => {
		const valid = await vm.process.execFile(
			"node",
			["--no-warnings", "-e", "console.log(42)"],
			{ timeoutMs: 10_000, output: { capture: "all" } },
		);
		expect(valid.exitCode, JSON.stringify(valid)).toBe(0);
		expect(valid.stdout).toBe("42\n");
		const invalid = await vm.process.execFile("node", ["--not-a-node-option"], {
			timeoutMs: 10_000,
			output: { capture: "all" },
		});
		expect(invalid.exitCode, JSON.stringify(invalid)).toBe(9);
		expect(invalid.stderr).toContain("bad option: --not-a-node-option");
	});

	test("synchronous descriptor reads preserve byte content", async () => {
		const result = await vm.javascript.execute(
			"import fs from 'node:fs'; console.log(fs.readFileSync(0).toString('hex'));",
			{
				stdin: new Uint8Array([0, 1, 127, 128, 255]),
				timeoutMs: 10_000,
				output: { capture: "all" },
			},
		);
		expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
		expect(result.stdout).toBe("00017f80ff\n");
	});

	test("a zero-byte read returns immediately with open stdin", async () => {
		const child = await vm.javascript.spawn(
			"import fs from 'node:fs'; console.log(fs.readSync(0, Buffer.alloc(1), 0, 0, null));",
			{ timeoutMs: 400, output: { capture: "all" } },
		);
		const exit = await vm.process.wait(child.pid);
		expect(exit.outcome).toBe("exited");
		expect(exit.exitCode).toBe(0);
	}, 5_000);

	test("a deadline interrupts a guest blocked on open stdin", async () => {
		const started = Date.now();
		const child = await vm.javascript.spawn(
			"import fs from 'node:fs'; fs.readFileSync(0, 'utf8');",
			{ timeoutMs: 400 },
		);
		const exit = await vm.process.wait(child.pid);
		expect(exit.outcome).toBe("timed_out");
		expect(Date.now() - started).toBeLessThan(3_000);
	}, 5_000);

	test("SIGKILL interrupts a guest blocked on open stdin", async () => {
		const child = await vm.javascript.spawn(
			"import fs from 'node:fs'; fs.readFileSync(0, 'utf8');",
		);
		const started = Date.now();
		await vm.process.kill(child.pid, "SIGKILL");
		const exit = await vm.process.wait(child.pid);
		expect(exit.exitCode).toBe(137);
		expect(Date.now() - started).toBeLessThan(3_000);
	}, 5_000);
});
