import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

describe("allProcesses()", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create();
	}, 30_000);

	afterEach(async () => {
		if (vm) {
			await vm.dispose();
		}
	}, 30_000);

	test("returns empty on a fresh VM with no spawned processes", async () => {
		const all = await vm.allProcesses();
		expect(all).toEqual([]);
	});

	test("spawned process appears in allProcesses alongside kernel processes", async () => {
		const before = await vm.allProcesses();
		await vm.writeFile("/tmp/stay.mjs", "setTimeout(() => {}, 30000);");
		const { pid } = await vm.spawn("node", ["/tmp/stay.mjs"], {
			env: { HOME: "/home/agentos" },
		});

		const after = await vm.allProcesses();
		expect(after.length).toBeGreaterThan(before.length);

		const found = after.find((p) => p.pid === pid);
		expect(found).toBeDefined();
		expect(found?.command).toBe("node");
		expect(found?.args).toEqual(["node", "/tmp/stay.mjs"]);
		expect(found?.cwd).toBe("/workspace");
		expect(found?.startTime).toBeGreaterThan(0);
		expect(found?.exitTime).toBeNull();

		await vm.killProcess(pid);
	}, 30_000);

	test("ppid relationships are correct", async () => {
		await vm.writeFile("/tmp/child.mjs", "setTimeout(() => {}, 30000);");
		const { pid } = await vm.spawn("node", ["/tmp/child.mjs"], {
			env: { HOME: "/home/agentos" },
		});

		const all = await vm.allProcesses();
		const child = all.find((p) => p.pid === pid);
		expect(child).toBeDefined();
		// ppid should reference an existing process (the kernel init or similar)
		expect(child?.ppid).toBeGreaterThanOrEqual(0);
		if (child?.ppid > 0) {
			const parent = all.find((p) => p.pid === child?.ppid);
			expect(parent).toBeDefined();
		}

		await vm.killProcess(pid);
	}, 30_000);

	test("guest child_process.spawn children appear in allProcesses()", async () => {
		let childPid: string | null = null;

		await vm.writeFile(
			"/tmp/parent.mjs",
			`
import { spawn } from "node:child_process";
const child = spawn("node", ["/tmp/child.mjs"]);
console.log("CHILD_PID:" + child.pid);
setTimeout(() => {}, 30000);
`,
		);
		await vm.writeFile("/tmp/child.mjs", "setTimeout(() => {}, 30000);");

		const { pid } = await vm.spawn("node", ["/tmp/parent.mjs"], {
			env: { HOME: "/home/agentos" },
			onStdout: (data) => {
				const text = new TextDecoder().decode(data);
				const match = text.match(/CHILD_PID:(\d+)/);
				if (match) {
					childPid = match[1];
				}
			},
		});

		for (let attempt = 0; attempt < 20 && childPid === null; attempt++) {
			await new Promise((resolve) => setTimeout(resolve, 100));
		}

		expect(childPid).not.toBeNull();

		let childProcess = null;
		for (let attempt = 0; attempt < 20; attempt++) {
			childProcess =
				(await vm.allProcesses()).find(
					(process) => process.pid === Number(childPid),
				) ?? null;
			if (childProcess) {
				break;
			}
			await new Promise((resolve) => setTimeout(resolve, 100));
		}

		expect(childProcess).toBeDefined();
		expect(childProcess?.command).toBe("node");
		expect(childProcess?.ppid).toBe(pid);

		await vm.killProcess(pid);
	}, 30_000);
});
