import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { EVALUATED_IN, type EvaluatedIn, runNode, waitForSnapshotBoot } from "./helpers/module-snapshot.js";

// A process reuses a module snapshot only while the snapshot's imports still
// resolve to the same files. These host changes alter what an import resolves
// to without editing any module in the snapshot.

const ENTRY = "/opt/app/src/entry.mjs";

let hostDir: string;
let vm: AgentOs;

function writePackage(dir: string, files: Record<string, string>): void {
	mkdirSync(dir, { recursive: true });
	for (const [name, content] of Object.entries(files)) {
		writeFileSync(join(dir, name), content);
	}
}

const runEntry = () => runNode<{ version: string } & EvaluatedIn>(vm, ENTRY);

const waitForSnapshot = (version: string) =>
	waitForSnapshotBoot(runEntry, (report) => report.version === version);

const depModule = (version: string) =>
	`export const version = ${JSON.stringify(version)};\n${EVALUATED_IN}`;

beforeAll(async () => {
	hostDir = mkdtempSync(join(tmpdir(), "agentos-module-snapshot-resolution-"));
	writePackage(join(hostDir, "node_modules", "dep"), {
		"package.json": JSON.stringify({ name: "dep", type: "module", exports: "./a.mjs" }),
		"a.mjs": depModule("exports-a"),
		"b.mjs": depModule("exports-b"),
	});
	writePackage(join(hostDir, "src"), {
		"entry.mjs":
			'import { version, evaluatedIn } from "dep";\nprocess.stdout.write(JSON.stringify({ version, evaluatedIn }));\n',
	});
	vm = await AgentOs.create({
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
}, 60_000);

afterAll(async () => {
	await vm?.dispose();
	rmSync(hostDir, { recursive: true, force: true });
});

test("an import that resolves to another file after a host change loads that file", async () => {
	await waitForSnapshot("exports-a");

	// Same files, new package.json `exports` target.
	writeFileSync(
		join(hostDir, "node_modules", "dep", "package.json"),
		JSON.stringify({ name: "dep", type: "module", exports: "./b.mjs" }),
	);
	expect((await runEntry()).version).toBe("exports-b");
	await waitForSnapshot("exports-b");

	// A closer node_modules now shadows the package.
	writePackage(join(hostDir, "src", "node_modules", "dep"), {
		"package.json": JSON.stringify({ name: "dep", type: "module", exports: "./index.mjs" }),
		"index.mjs": depModule("shadow"),
	});
	expect((await runEntry()).version).toBe("shadow");
}, 120_000);
