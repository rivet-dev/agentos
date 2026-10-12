import common from "@agentos-software/common";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, hostFunction, hostFunctions } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

const mathFunctions = {
	add: {
		inputSchema: z
			.object({
				a: z.number(),
				b: z.number(),
			})
			.describe("Add two numbers"),
		execute: ({ a, b }) => ({ sum: a + b }),
	},
};

async function runCommand(vm: AgentOs, command: string, args: string[]) {
	return vm.process.execFile(command, args, { output: { capture: "all" } });
}

describe("native sidecar hostFunction dispatch", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			software: [common],
			hostFunctions: { math: mathFunctions },
			permissions: ALLOW_ALL_VM_PERMISSIONS,
		});
	}, 20_000);

	afterEach(async () => {
		await vm?.dispose();
	});

	test("agentos list-host-functions returns registered host-function collections", async () => {
		const result = await runCommand(vm, "agentos", ["list-host-functions"]);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: {
				hostFunctions: [
					{
						name: "math",
						functions: ["add"],
					},
				],
			},
		});
	});

	test("agentos-<hostFunction collection> executes the hostFunction through the sidecar", async () => {
		const result = await runCommand(vm, "agentos-math", [
			"add",
			"--a",
			"5",
			"--b",
			"7",
		]);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: { sum: 12 },
		});
	});

	test("guest shell scripts can invoke agentos-* commands through PATH", async () => {
		await vm.writeFile(
			"/tmp/run-hostFunction.sh",
			[
				"#!/bin/sh",
				"set -eu",
				"agentos-math add --a 2 --b 3 > /tmp/hostFunction-output.json",
			].join("\n"),
		);

		const result = await vm.exec(
			"sh /tmp/run-hostFunction.sh && cat /tmp/hostFunction-output.json",
		);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: { sum: 5 },
		});
	});

	test("external shell commands preserve duplicated stdio and redirection order", async () => {
		const cases = [
			{
				command: "echo shell-check | cat 1>&2",
				stdout: "",
				stderr: "shell-check\n",
			},
			{
				command: "printf O 3>&1 1>&2 2>&3 3>&-",
				stdout: "",
				stderr: "O",
			},
			{
				command: "sh -c 'printf O; printf E >&2' 3>&1 2>&3 3>&-",
				stdout: "OE",
				stderr: "",
			},
			{
				command: "sh -c 'printf O; printf E >&2' 3>&1 1>&2 2>&3",
				stdout: "E",
				stderr: "O",
			},
			{
				command: "printf O | cat 3>&1 1>&2 2>&3 3>&-",
				stdout: "",
				stderr: "O",
			},
			{
				command: "sh -c 'printf O; printf E >&2' 2>&1",
				stdout: "OE",
				stderr: "",
			},
			{
				command: "sh -c 'printf O; printf E >&2' 1>&2 2>&1",
				stdout: "",
				stderr: "OE",
			},
			{
				command: "sh -c 'printf O; printf E >&2' 3>&1 1>&2 2>&3 3>&-",
				stdout: "E",
				stderr: "O",
			},
		];
		for (const { command, stdout, stderr } of cases) {
			const result = await vm.process.exec(command, {
				output: { capture: "all" },
			});
			expect(result.exitCode, `${command}: ${JSON.stringify(result)}`).toBe(0);
			expect(result.stdout, command).toBe(stdout);
			expect(result.stderr, command).toBe(stderr);
		}
	});

	test("host-function output reaches composed shell commands with restricted environment access", async () => {
		await vm.dispose();
		vm = await AgentOs.create({
			software: [common],
			hostFunctions: { math: mathFunctions },
			permissions: {
				fs: "allow",
				childProcess: "allow",
				process: "allow",
				hostFunction: "allow",
				network: "deny",
				env: "deny",
			},
		});
		const command = 'agentos-math add --json \'{"a":2,"b":3}\'';
		const options = {
			cwd: "/",
			timeoutMs: 10_000,
			output: { capture: "all" as const },
		};
		const direct = await vm.process.exec(command, options);
		expect(direct.exitCode, JSON.stringify(direct)).toBe(0);
		expect(JSON.parse(direct.stdout)).toEqual({ ok: true, result: { sum: 5 } });
		for (const composed of [
			`${command} | wc -c`,
			`${command} > /tmp/host-function-output.json; wc -c < /tmp/host-function-output.json`,
		]) {
			const result = await vm.process.exec(composed, options);
			expect(result.exitCode, JSON.stringify(result)).toBe(0);
			expect(Number(result.stdout.trim())).toBe(
				new TextEncoder().encode(direct.stdout).length,
			);
		}
		const substitution = await vm.process.exec(
			`value="$(${command})"; printf '[%s]\\n' "$value"`,
			options,
		);
		expect(substitution.exitCode, JSON.stringify(substitution)).toBe(0);
		expect(substitution.stdout).toBe(`[${direct.stdout.trim()}]\n`);
	});

	test("invalid hostFunction input exits non-zero and writes the error to stderr", async () => {
		const result = await runCommand(vm, "agentos-math", ["add", "--a", "5"]);
		expect(result.exitCode).toBe(1);
		expect(result.stderr).toContain("Missing required flag");
		expect(result.stderr).toContain("--b");
	});
});
