import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import {
	EVALUATED_IN,
	type EvaluatedIn,
	LIVE_ENV,
	runNode,
	waitForSnapshotBoot,
} from "./helpers/module-snapshot.js";

// Nightly: saturates the sidecar's 64 MiB bound on shared snapshot sources.
//
// Finding a graph's imports must not hold its sources. Here three graphs of
// 30 MiB each are found but never snapshotted (their top-level code throws
// in the snapshot helper), so no snapshot uses their sources. A graph found
// after them must still get a snapshot.

const LARGE_GRAPHS = 3;
const LARGE_MODULE_BYTES = 30 * 1024 * 1024;

let hostDir: string;
let vm: AgentOs;

const entryPath = (name: string) => `/opt/app/${name}-entry.mjs`;
const run = (name: string) => runNode<EvaluatedIn>(vm, entryPath(name));

function writeGraph(name: string, source: string): void {
	writeFileSync(join(hostDir, `${name}.mjs`), `${source}\n${EVALUATED_IN}`);
	writeFileSync(
		join(hostDir, `${name}-entry.mjs`),
		`import { evaluatedIn } from "./${name}.mjs";\nprocess.stdout.write(JSON.stringify({ evaluatedIn }));\n`,
	);
}

beforeAll(async () => {
	hostDir = mkdtempSync(join(tmpdir(), "agentos-module-snapshot-sources-"));
	const names: string[] = [];
	for (let index = 0; index < LARGE_GRAPHS; index += 1) {
		// Distinct content per graph, so no two share a source.
		const padding = `// ${index} ${"x".repeat(LARGE_MODULE_BYTES)}`;
		writeGraph(
			`large-${index}`,
			`${padding}\nif (process.env.${LIVE_ENV} === undefined) throw new Error("not snapshotted");`,
		);
		writeGraph(`barrier-${index}`, "");
		names.push(`large-${index}`, `barrier-${index}`);
	}
	// Large enough (> 64 KiB) that its snapshot shares the source.
	writeGraph("after", `// ${"y".repeat(6 * 1024 * 1024)}`);
	names.push("after");
	vm = await AgentOs.create({
		permissions: { network: "deny" },
		snapshotEntrypoints: names.map(entryPath),
		mounts: [
			{
				path: "/opt/app",
				readOnly: true,
				plugin: { id: "host_dir", config: { hostPath: hostDir, readOnly: true } },
			},
		],
	});
}, 60_000);

afterAll(async () => {
	await vm?.dispose();
	rmSync(hostDir, { recursive: true, force: true });
});

test("after finding more than 64 MiB of large graph sources, a new graph still boots from a snapshot", async () => {
	for (let index = 0; index < LARGE_GRAPHS; index += 1) {
		expect(await run(`large-${index}`)).toEqual({ evaluatedIn: "live" });
		// Snapshot builds run one at a time, so the large graph's build is
		// done once a graph launched after it boots from a snapshot.
		await waitForSnapshotBoot(() => run(`barrier-${index}`), undefined, 60_000);
	}
	await waitForSnapshotBoot(() => run("after"), undefined, 60_000);
}, 300_000);
