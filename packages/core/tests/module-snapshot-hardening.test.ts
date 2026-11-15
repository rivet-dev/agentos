import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import {
	EVALUATED_IN,
	type EvaluatedIn,
	LIVE_ENV,
	runNode,
	waitForSnapshotBoot,
} from "./helpers/module-snapshot.js";

// Module snapshot builds evaluate guest code once for every process that
// boots from the result. These tests check that such code can neither hold
// the build forever nor freeze per-process values into what other processes
// and VMs see.

// The sidecar this file starts kills a snapshot helper after this long.
const HELPER_TIMEOUT_MS = 8_000;
process.env.AGENTOS_V8_SNAPSHOT_HELPER_TIMEOUT_MS = String(HELPER_TIMEOUT_MS);

// Top-level reads that differ per process.
const PER_PROCESS_VALUES = [
	"Math.random()",
	"Date.now()",
	"new Date().getTime()",
	"Date()",
	"performance.now()",
	"process.hrtime()[1]",
	"String(process.hrtime.bigint())",
	"process.uptime()",
	"crypto.randomUUID()",
];

type Report = EvaluatedIn & { value?: unknown; sample?: Record<string, unknown> };

let hostDir: string;
const vms: AgentOs[] = [];

/** Write `<name>.mjs` and an entry that prints what it exports. */
function writeGraph(name: string, source: string, report = "{ evaluatedIn, value }"): string {
	writeFileSync(join(hostDir, `${name}.mjs`), `${source}\n${EVALUATED_IN}`);
	const imports = report.includes("sample") ? "evaluatedIn, sample" : report.includes("value") ? "evaluatedIn, value" : "evaluatedIn";
	writeFileSync(
		join(hostDir, `${name}-entry.mjs`),
		`import { ${imports} } from "./${name}.mjs";\nprocess.stdout.write(JSON.stringify(${report}));\n`,
	);
	return `/opt/app/${name}-entry.mjs`;
}

async function createVm(entries: string[]): Promise<AgentOs> {
	const vm = await AgentOs.create({
		permissions: { network: "deny" },
		snapshotEntrypoints: entries,
		mounts: [
			{
				path: "/opt/app",
				readOnly: true,
				plugin: { id: "host_dir", config: { hostPath: hostDir, readOnly: true } },
			},
		],
	});
	vms.push(vm);
	return vm;
}

const run = (vm: AgentOs, entry: string) => runNode<Report>(vm, entry);

describe("module snapshot builds of hostile graphs", () => {
	beforeAll(() => {
		hostDir = mkdtempSync(join(tmpdir(), "agentos-module-snapshot-hardening-"));
	});

	afterAll(async () => {
		await Promise.all(vms.map((vm) => vm.dispose()));
		rmSync(hostDir, { recursive: true, force: true });
	});

	test("a graph whose top-level code never finishes is killed, and other graphs still snapshot", async () => {
		// Never finishes, but only in the snapshot helper: a process sets LIVE_ENV.
		const spin = writeGraph("spin", `if (process.env.${LIVE_ENV} === undefined) { for (;;) {} }`, "{ evaluatedIn }");
		const clean = writeGraph("clean", "", "{ evaluatedIn }");
		const first = await createVm([spin, clean]);
		// Starts the spinning build; the process itself loads the module normally.
		expect(await run(first, spin)).toEqual({ evaluatedIn: "live" });

		// The next build waits for the spinning one, which the deadline ends.
		// A VM created meanwhile starts, and both VMs boot from the new snapshot.
		const second = await createVm([spin, clean]);
		await waitForSnapshotBoot(() => run(second, clean), undefined, HELPER_TIMEOUT_MS + 30_000);
		expect(await run(first, clean)).toEqual({ evaluatedIn: "snapshot" });
	}, 90_000);

	test.each(PER_PROCESS_VALUES)(
		"processes in two VMs never share a top-level %s",
		async (expression) => {
			const index = PER_PROCESS_VALUES.indexOf(expression);
			const valueEntry = writeGraph(`value-${index}`, `export const value = ${expression};`);
			const barrier = writeGraph(`barrier-${index}`, "", "{ evaluatedIn }");
			const [first, second] = [await createVm([valueEntry, barrier]), await createVm([valueEntry, barrier])];
			// Starts the value graph's build first. Once a graph launched after
			// it boots from a snapshot, that build has run.
			const values = [(await run(first, valueEntry)).value];
			await waitForSnapshotBoot(() => run(first, barrier));

			for (const vm of [first, second, first, second]) {
				const report = await run(vm, valueEntry);
				expect(report.evaluatedIn).toBe("live");
				values.push(report.value);
			}
			if (expression === "Math.random()" || expression === "crypto.randomUUID()") {
				expect(new Set(values).size).toBe(values.length);
			}
		},
		60_000,
	);

	test("a graph that keeps the clock and RNG for later, or checks performance.timeOrigin, still snapshots and reads live values", async () => {
		const captured = writeGraph(
			"captured",
			`const SavedDate = Date;
const now = Date.now;
const random = Math.random;
const hrtime = process.hrtime;
// Feature detection as a Workers runtime does it, at module top level.
if (typeof performance.timeOrigin !== "number") throw new Error("no performance.timeOrigin");
export function sample() {
	return {
		now: now(),
		date: new SavedDate().getTime(),
		random: random(),
		hrtime: hrtime.bigint() > 0n,
		timeOrigin: performance.timeOrigin,
		toJSON: typeof performance.toJSON().timeOrigin === "number",
	};
}`,
			"{ evaluatedIn, sample: sample() }",
		);
		const vm = await createVm([captured]);
		await waitForSnapshotBoot(() => run(vm, captured));

		const [one, two] = [await run(vm, captured), await run(vm, captured)];
		for (const report of [one, two]) {
			expect(report.evaluatedIn).toBe("snapshot");
			expect(Math.abs((report.sample?.now as number) - Date.now())).toBeLessThan(60_000);
			expect(Math.abs((report.sample?.date as number) - Date.now())).toBeLessThan(60_000);
			expect(report.sample?.hrtime).toBe(true);
			// A live per-process value, as in Node.
			expect(Math.abs((report.sample?.timeOrigin as number) - Date.now())).toBeLessThan(5_000);
			expect(report.sample?.toJSON).toBe(true);
		}
		expect(one.sample?.random).not.toBe(two.sample?.random);
	}, 90_000);
});
