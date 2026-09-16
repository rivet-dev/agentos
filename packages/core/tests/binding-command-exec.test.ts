import common from "@agentos-software/common";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, binding, bindings } from "../src/index.js";

// Regression coverage for rivet-dev/agentos#1885: the `agentos-<collection>`
// command stub must dispatch to the host binding when it is executed the way
// agents run it, through the shell and a PATH lookup, not only via a direct
// `spawn`.

const weatherBindings = bindings({
	name: "weather",
	description: "Weather data bindings",
	bindings: {
		forecast: binding({
			description: "Get a forecast",
			inputSchema: z.object({ city: z.string() }).strict(),
			execute: ({ city }) => ({ city, temperature: 22 }),
		}),
	},
});

const EXPECTED_ENVELOPE = {
	ok: true,
	result: { city: "Paris", temperature: 22 },
};

// `process.exec()` runs the command line through the guest shell, which
// resolves `agentos-weather` through PATH to the `/bin/agentos-weather` stub and
// spawns that exact path.
const COMMANDS = [
	"agentos-weather forecast --city Paris",
	"/bin/agentos-weather forecast --city Paris",
	"cd /tmp && agentos-weather forecast --city Paris",
];

async function execCapture(vm: AgentOs, command: string) {
	const decoder = new TextDecoder();
	let stdout = "";
	let stderr = "";
	const result = await vm.process.exec(command, {
		onStdout: (chunk) => {
			stdout += decoder.decode(chunk);
		},
		onStderr: (chunk) => {
			stderr += decoder.decode(chunk);
		},
	});
	return { exitCode: result.exitCode, stdout, stderr };
}

describe.each([
	{
		label: "without extra software",
		options: {
			permissions: {
				fs: "allow",
				childProcess: "allow",
				process: "allow",
				env: "allow",
				binding: "allow",
				network: "deny",
			},
		},
	},
	{
		label: "with the common software package",
		options: { software: [common] },
	},
] as const)("binding command exec ($label)", ({ options }) => {
	let vm: AgentOs;

	beforeAll(async () => {
		vm = await AgentOs.create({
			...options,
			bindings: [weatherBindings],
		} as Parameters<typeof AgentOs.create>[0]);
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
	});

	test("stub is installed at /bin/agentos-weather", async () => {
		expect(await vm.exists("/bin/agentos-weather")).toBe(true);
	});

	test.each(
		COMMANDS,
	)("`%s` dispatches to the host binding", async (command) => {
		const result = await execCapture(vm, command);
		expect(
			{ exitCode: result.exitCode, stderr: result.stderr },
			`stdout: ${result.stdout}`,
		).toEqual({ exitCode: 0, stderr: "" });
		expect(JSON.parse(result.stdout)).toEqual(EXPECTED_ENVELOPE);
	});

	test("`agentos list-bindings` runs through the shell", async () => {
		const result = await execCapture(vm, "agentos list-bindings");
		expect(
			{ exitCode: result.exitCode, stderr: result.stderr },
			`stdout: ${result.stdout}`,
		).toEqual({ exitCode: 0, stderr: "" });
		expect(JSON.parse(result.stdout)).toEqual({
			ok: true,
			result: {
				bindings: [
					{
						name: "weather",
						description: "Weather data bindings",
						bindings: ["forecast"],
					},
				],
			},
		});
	});
});
