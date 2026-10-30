import { afterEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

// Every guest Node process in a VM with host functions gets one frozen global
// per collection, not only inline `vm.javascript.*` executions. These tests
// launch Node the other ways a guest can: a direct spawn and a child process
// of guest Node. The shell launch paths need the default software bundle, so
// they live in host-function-globals.nightly.test.ts.

const orderStore = {
	listOrders: {
		inputSchema: z.object({ status: z.string() }),
		execute: ({ status }: { status: string }) => ({
			orders: [{ id: "order-1", status }],
		}),
	},
};

const EXPECTED_RESULT = { orders: [{ id: "order-1", status: "open" }] };

const MAIN_SCRIPT = `
const result = await orderStore.listOrders({ status: "open" });
console.log(JSON.stringify(result));
`;

async function runNode(
	vm: AgentOs,
	command: string,
	args: string[],
): Promise<{ exitCode?: number; stdout: string; stderr: string }> {
	const decoder = new TextDecoder();
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn(command, args, {
		onStdout: (chunk) => {
			stdout += decoder.decode(chunk);
		},
		onStderr: (chunk) => {
			stderr += decoder.decode(chunk);
		},
	});
	const exit = await vm.process.wait(pid);
	return { exitCode: exit.exitCode, stdout, stderr };
}

function lastJsonLine(stdout: string): unknown {
	const lines = stdout.trim().split("\n");
	return JSON.parse(lines[lines.length - 1] ?? "");
}

describe("host-function globals in guest Node processes", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	test("spawned and child Node processes can call host functions", async () => {
		vm = await AgentOs.create({
			permissions: ALLOW_ALL_VM_PERMISSIONS,
			hostFunctions: { orderStore },
		});
		await vm.writeFile("/app/main.mjs", MAIN_SCRIPT);
		await vm.writeFile(
			"/app/main.cjs",
			`orderStore.listOrders({ status: "open" }).then((result) => {
				console.log(JSON.stringify(result));
			});`,
		);
		// A static import evaluates before the importing module's body, so this
		// also covers modules that use the global at their top level.
		await vm.writeFile(
			"/app/orders.mjs",
			`export const result = await orderStore.listOrders({ status: "open" });`,
		);
		await vm.writeFile(
			"/app/import.mjs",
			`import { result } from "./orders.mjs";
console.log(JSON.stringify(result));`,
		);
		await vm.writeFile(
			"/app/parent.mjs",
			`import { execFile } from "node:child_process";
execFile("node", ["/app/main.mjs"], (error, stdout, stderr) => {
	if (error) {
		console.error(stderr || error.message);
		process.exit(1);
	}
	process.stdout.write(stdout);
});`,
		);

		await vm.writeFile(
			"/app/grandparent.mjs",
			`import { execFile } from "node:child_process";
execFile("node", ["/app/parent.mjs"], (error, stdout, stderr) => {
    if (error) {
        console.error(stderr || error.message);
        process.exit(1);
    }
    process.stdout.write(stdout);
});`,
		);

		const cases: Array<[string, string, string[]]> = [
			["process.spawn ESM", "node", ["/app/main.mjs"]],
			["process.spawn CommonJS", "node", ["/app/main.cjs"]],
			["process.spawn static import", "node", ["/app/import.mjs"]],
			["guest child_process", "node", ["/app/parent.mjs"]],
			["nested guest child_process", "node", ["/app/grandparent.mjs"]],
		];
		for (const [label, command, args] of cases) {
			const result = await runNode(vm, command, args);
			expect(
				{ label, exitCode: result.exitCode, stderr: result.stderr },
				`stdout: ${result.stdout}`,
			).toEqual({ label, exitCode: 0, stderr: "" });
			expect(lastJsonLine(result.stdout), label).toEqual(EXPECTED_RESULT);
		}

		// The generated CLI keeps working alongside the globals.
		const cli = await runNode(vm, "agentos-order-store", [
			"list-orders",
			"--status",
			"open",
		]);
		expect(cli.exitCode, cli.stderr).toBe(0);
		expect(JSON.parse(cli.stdout)).toEqual({
			ok: true,
			result: EXPECTED_RESULT,
		});
	}, 120_000);

	test("inline executions keep the globals across a retained context", async () => {
		vm = await AgentOs.create({
			permissions: ALLOW_ALL_VM_PERMISSIONS,
			hostFunctions: { orderStore },
		});
		await vm.createContext("orders");
		for (let run = 0; run < 2; run++) {
			const result = await vm.javascript.evaluate(
				`orderStore.listOrders({ status: "open" })`,
				{ contextId: "orders" },
			);
			expect(result, `run ${run}`).toMatchObject({
				exitCode: 0,
				value: EXPECTED_RESULT,
			});
			expect(JSON.stringify(result), `run ${run}`).not.toContain(
				"was not defined",
			);
		}
	}, 60_000);

	test("the hostFunction permission still applies to the globals", async () => {
		vm = await AgentOs.create({
			permissions: {
				...ALLOW_ALL_VM_PERMISSIONS,
				hostFunction: { default: "deny", rules: [] },
			},
			hostFunctions: { orderStore },
		});
		await vm.writeFile(
			"/app/denied.mjs",
			`try {
	await orderStore.listOrders({ status: "open" });
	console.log("ALLOWED");
} catch (error) {
	console.log("DENIED " + error.message);
}`,
		);

		const result = await runNode(vm, "node", ["/app/denied.mjs"]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout).toContain("DENIED");
		expect(result.stdout).toContain("hostFunction.invoke");
		expect(result.stdout).toContain("order-store:list-orders");
	}, 60_000);

	test("no globals are defined without host functions", async () => {
		vm = await AgentOs.create({ permissions: ALLOW_ALL_VM_PERMISSIONS });
		await vm.writeFile(
			"/app/types.mjs",
			`console.log(JSON.stringify(typeof globalThis.orderStore));`,
		);

		const result = await runNode(vm, "node", ["/app/types.mjs"]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stderr).toBe("");
		expect(lastJsonLine(result.stdout)).toBe("undefined");
	}, 60_000);

	test("a collection named like an existing global is skipped with a warning", async () => {
		vm = await AgentOs.create({
			permissions: ALLOW_ALL_VM_PERMISSIONS,
			hostFunctions: {
				process: {
					status: {
						inputSchema: z.object({}),
						execute: () => ({ ok: true }),
					},
				},
				orderStore,
			},
		});
		await vm.writeFile(
			"/app/collision.mjs",
			`console.log(JSON.stringify({
	processStatus: typeof process.status,
	cwd: typeof process.cwd,
	orderStore: typeof orderStore.listOrders,
}));`,
		);

		const result = await runNode(vm, "node", ["/app/collision.mjs"]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stderr).toContain(
			'agentos: host function group "process" was not defined because globalThis.process already exists',
		);
		expect(lastJsonLine(result.stdout)).toEqual({
			processStatus: "undefined",
			cwd: "function",
			orderStore: "function",
		});
	}, 60_000);
});
