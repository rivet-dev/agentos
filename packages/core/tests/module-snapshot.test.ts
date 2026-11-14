import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// A guest `node <entry>` process for a snapshot entrypoint boots with the
// entry's imports evaluated once, in a V8 startup snapshot. These tests drive
// that through a real sidecar and check what each process observes.

const ENTRY = "/opt/app/entry.mjs";

// Big enough (> 64 KiB) that the snapshot shares the source text across
// isolates. `’` makes it a two-byte string.
const PADDING = `// padding ’ ${"x".repeat(70 * 1024)}\n`;

function libSource(version: string): string {
	return `${PADDING}import { state } from "./state.mjs";
// Computed when the module evaluates: once per snapshot, not per process.
export const loadedToken = Math.random();
const VERSION = ${JSON.stringify(version)};
export function report() {
	state.count += 1;
	return {
		argv: process.argv.slice(2),
		foo: process.env.FOO ?? null,
		cwd: process.cwd(),
		random: Math.random(),
		loadedToken,
		count: state.count,
		version: VERSION,
		source: report.toString().includes("state.count += 1"),
	};
}
`;
}

interface Report {
	argv: string[];
	foo: string | null;
	cwd: string;
	random: number;
	loadedToken: number;
	count: number;
	version: string;
	source: boolean;
}

async function runEntry(
	vm: AgentOs,
	args: string[] = [],
	options: { env?: Record<string, string>; cwd?: string } = {},
): Promise<Report> {
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn("node", [ENTRY, ...args], {
		...options,
		onStdout: (chunk) => {
			stdout += Buffer.from(chunk).toString();
		},
		onStderr: (chunk) => {
			stderr += Buffer.from(chunk).toString();
		},
	});
	const exit = await vm.process.wait(pid);
	expect({ exitCode: exit.exitCode, stderr }).toEqual({ exitCode: 0, stderr: "" });
	return JSON.parse(stdout) as Report;
}

// The snapshot builds in the background after the first launch. Once two
// processes report the same import-time token, they booted from it.
async function waitForSnapshot(vm: AgentOs): Promise<number> {
	const deadline = Date.now() + 30_000;
	let previous = (await runEntry(vm)).loadedToken;
	while (Date.now() < deadline) {
		await new Promise((resolve) => setTimeout(resolve, 200));
		const next = (await runEntry(vm)).loadedToken;
		if (next === previous) return next;
		previous = next;
	}
	throw new Error("guest processes never booted from the module snapshot");
}

describe("module snapshots", () => {
	let hostDir: string;
	let first: AgentOs;
	let second: AgentOs;

	const createVm = () =>
		AgentOs.create({
			permissions: { network: "deny" },
			snapshotEntrypoints: [ENTRY],
			mounts: [
				{
					path: "/opt/app",
					readOnly: true,
					plugin: { id: "host_dir", config: { hostPath: hostDir, readOnly: true } },
				},
			],
		});

	beforeAll(async () => {
		hostDir = mkdtempSync(join(tmpdir(), "agentos-module-snapshot-"));
		writeFileSync(join(hostDir, "state.mjs"), "export const state = { count: 0 };\n");
		writeFileSync(join(hostDir, "lib.mjs"), libSource("v1"));
		writeFileSync(
			join(hostDir, "entry.mjs"),
			'import { report } from "./lib.mjs";\nprocess.stdout.write(JSON.stringify(report()));\n',
		);
		first = await createVm();
		second = await createVm();
	}, 60_000);

	afterAll(async () => {
		await first?.dispose();
		await second?.dispose();
		rmSync(hostDir, { recursive: true, force: true });
	});

	test("a snapshot-booted process sees its own argv, env, cwd, and randomness", async () => {
		const token = await waitForSnapshot(first);

		const one = await runEntry(first, ["a", "b"], { env: { FOO: "one" }, cwd: "/tmp" });
		const two = await runEntry(first, ["c"], { env: { FOO: "two" }, cwd: "/" });

		expect(one).toMatchObject({
			argv: ["a", "b"],
			foo: "one",
			cwd: "/tmp",
			loadedToken: token,
			count: 1,
			source: true,
		});
		expect(two).toMatchObject({ argv: ["c"], foo: "two", cwd: "/", loadedToken: token, count: 1 });
		expect(one.random).not.toBe(two.random);
	}, 90_000);

	test("concurrent processes in two VMs share no module state", async () => {
		const token = await waitForSnapshot(first);

		const reports = await Promise.all([
			runEntry(first, ["first-0"]),
			runEntry(second, ["second-0"]),
			runEntry(first, ["first-1"]),
			runEntry(second, ["second-1"]),
		]);

		expect(reports.map((report) => report.argv[0])).toEqual([
			"first-0",
			"second-0",
			"first-1",
			"second-1",
		]);
		// Both VMs boot from the one snapshot, yet each process starts from its
		// own copy of the module state.
		for (const report of reports) {
			expect(report).toMatchObject({ count: 1, version: "v1", loadedToken: token });
		}
		expect(new Set(reports.map((report) => report.random)).size).toBe(reports.length);
	}, 90_000);

	test("changing an imported module on the host takes effect on the next process", async () => {
		const before = await waitForSnapshot(first);
		writeFileSync(join(hostDir, "lib.mjs"), libSource("v2"));

		const next = await runEntry(first);
		expect(next.version).toBe("v2");

		// The rebuilt snapshot carries the new module.
		const after = await waitForSnapshot(first);
		expect(after).not.toBe(before);
		expect(await runEntry(second)).toMatchObject({ version: "v2", count: 1 });
	}, 90_000);
});
