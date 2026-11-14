import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

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

async function runEntry(): Promise<{ version: string; token: number }> {
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn("node", [ENTRY], {
		onStdout: (chunk) => {
			stdout += Buffer.from(chunk).toString();
		},
		onStderr: (chunk) => {
			stderr += Buffer.from(chunk).toString();
		},
	});
	const exit = await vm.process.wait(pid);
	expect({ exitCode: exit.exitCode, stderr }).toEqual({ exitCode: 0, stderr: "" });
	return JSON.parse(stdout);
}

// Once two processes report the same import-time token, they booted from the
// snapshot that the first launch builds in the background.
async function waitForSnapshot(): Promise<void> {
	const deadline = Date.now() + 30_000;
	let previous = (await runEntry()).token;
	while (Date.now() < deadline) {
		await new Promise((resolve) => setTimeout(resolve, 200));
		const next = (await runEntry()).token;
		if (next === previous) return;
		previous = next;
	}
	throw new Error("guest processes never booted from the module snapshot");
}

const depModule = (version: string) =>
	`export const version = ${JSON.stringify(version)};\nexport const token = Math.random();\n`;

beforeAll(async () => {
	hostDir = mkdtempSync(join(tmpdir(), "agentos-module-snapshot-resolution-"));
	writePackage(join(hostDir, "node_modules", "dep"), {
		"package.json": JSON.stringify({ name: "dep", type: "module", exports: "./a.mjs" }),
		"a.mjs": depModule("exports-a"),
		"b.mjs": depModule("exports-b"),
	});
	writePackage(join(hostDir, "src"), {
		"entry.mjs":
			'import { version, token } from "dep";\nprocess.stdout.write(JSON.stringify({ version, token }));\n',
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
	await waitForSnapshot();
	expect((await runEntry()).version).toBe("exports-a");

	// Same files, new package.json `exports` target.
	writeFileSync(
		join(hostDir, "node_modules", "dep", "package.json"),
		JSON.stringify({ name: "dep", type: "module", exports: "./b.mjs" }),
	);
	expect((await runEntry()).version).toBe("exports-b");
	await waitForSnapshot();

	// A closer node_modules now shadows the package.
	writePackage(join(hostDir, "src", "node_modules", "dep"), {
		"package.json": JSON.stringify({ name: "dep", type: "module", exports: "./index.mjs" }),
		"index.mjs": depModule("shadow"),
	});
	expect((await runEntry()).version).toBe("shadow");
}, 120_000);
