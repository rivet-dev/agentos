import common from "@agentos-software/common";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, binding, bindings } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

const mathBindings = bindings({
	name: "math",
	description: "Math utilities",
	bindings: {
		add: binding({
			description: "Add two numbers",
			inputSchema: z.object({
				a: z.number(),
				b: z.number(),
			}),
			execute: ({ a, b }) => ({ sum: a + b }),
		}),
	},
});

async function runCommand(vm: AgentOs, command: string, args: string[]) {
	const stdoutChunks: string[] = [];
	const stderrChunks: string[] = [];
	const { pid } = vm.spawn(command, args, {
		onStdout: (chunk) => {
			stdoutChunks.push(new TextDecoder().decode(chunk));
		},
		onStderr: (chunk) => {
			stderrChunks.push(new TextDecoder().decode(chunk));
		},
	});

	return {
		exitCode: await vm.waitProcess(pid),
		stdout: stdoutChunks.join(""),
		stderr: stderrChunks.join(""),
	};
}

describe("native sidecar tool dispatch", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			software: [common],
			bindings: [mathBindings],
			permissions: ALLOW_ALL_VM_PERMISSIONS,
		});
	}, 20_000);

	afterEach(async () => {
		await vm?.dispose();
	});

	test("agentos list-tools returns registered bindings", async () => {
		const result = await runCommand(vm, "agentos", ["list-tools"]);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: {
				bindings: [
					{
						name: "math",
						description: "Math utilities",
						bindings: ["add"],
					},
				],
			},
		});
	});

	test("agentos-<bindings> executes the binding through the sidecar", async () => {
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
			"/tmp/run-tool.sh",
			[
				"#!/bin/sh",
				"set -eu",
				"agentos-math add --a 2 --b 3 > /tmp/tool-output.json",
			].join("\n"),
		);

		const result = await vm.exec(
			"sh /tmp/run-tool.sh && cat /tmp/tool-output.json",
		);
		expect(result.exitCode).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: { sum: 5 },
		});
	});

	test("invalid binding input exits non-zero and writes the error to stderr", async () => {
		const result = await runCommand(vm, "agentos-math", ["add", "--a", "5"]);
		expect(result.exitCode).toBe(1);
		expect(result.stderr).toContain("Missing required flag");
		expect(result.stderr).toContain("--b");
	});
});
