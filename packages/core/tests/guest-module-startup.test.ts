import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function runNode(
	vm: AgentOs,
	args: string[],
	env: Record<string, string> = {},
): Promise<{ exitCode: number | undefined; stdout: string; stderr: string }> {
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn("node", args, {
		env,
		onStdout: (chunk) => {
			stdout += Buffer.from(chunk).toString();
		},
		onStderr: (chunk) => {
			stderr += Buffer.from(chunk).toString();
		},
	});
	const exit = await vm.process.wait(pid);
	return { exitCode: exit.exitCode, stdout, stderr };
}

// Large enough for the sidecar's module code cache, and the same length for
// every marker so only the content differs between VMs.
function moduleWithMarker(marker: string): string {
	const padding = `// ${"x".repeat(4096)}\n`;
	return `${padding}export const marker = ${JSON.stringify(marker)};\n`;
}

describe("guest module startup", () => {
	let first: AgentOs;
	let second: AgentOs;

	beforeAll(async () => {
		first = await AgentOs.create({ permissions: { network: "deny" } });
		second = await AgentOs.create({ permissions: { network: "deny" } });
	}, 60_000);

	afterAll(async () => {
		await first?.dispose();
		await second?.dispose();
	});

	test("VMs that import different code at the same path each run their own code", async () => {
		const main =
			"import { marker } from './shared.mjs';\nprocess.stdout.write(marker);\n";
		for (const [vm, marker] of [
			[first, "first-vm"],
			[second, "secondvm"],
		] as const) {
			await vm.writeFile("/tmp/shared.mjs", moduleWithMarker(marker));
			await vm.writeFile("/tmp/main.mjs", main);
		}

		// Each VM runs twice so the second start of each consumes whatever the
		// other VM's identical-path, identical-length module left behind.
		for (let round = 0; round < 2; round++) {
			const firstRun = await runNode(first, ["/tmp/main.mjs"]);
			const secondRun = await runNode(second, ["/tmp/main.mjs"]);
			expect(firstRun).toMatchObject({ exitCode: 0, stdout: "first-vm" });
			expect(secondRun).toMatchObject({ exitCode: 0, stdout: "secondvm" });
		}
	}, 60_000);

	test("an imported module sees the process argv and env at its top level", async () => {
		await first.writeFile(
			"/tmp/dep.mjs",
			"export const seen = { argv: process.argv.slice(2), foo: process.env.FOO ?? null };\n",
		);
		await first.writeFile(
			"/tmp/entry.mjs",
			"import { seen } from './dep.mjs';\nprocess.stdout.write(JSON.stringify(seen));\n",
		);

		const run = await runNode(first, ["/tmp/entry.mjs", "a", "b"], {
			FOO: "bar",
		});

		expect(run.exitCode).toBe(0);
		expect(JSON.parse(run.stdout)).toEqual({ argv: ["a", "b"], foo: "bar" });
	}, 60_000);
});
