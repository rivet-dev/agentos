import { spawnSync } from "node:child_process";
import {
	existsSync,
	readFileSync,
	readdirSync,
	writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const registryRoot = dirname(fileURLToPath(new URL("../package.json", import.meta.url)));
const repoRoot = resolve(registryRoot, "..");
const baselinePath = join(registryRoot, "test-status.json");
const runVitest = join(registryRoot, "scripts", "run-vitest.mjs");
const checkMode = process.argv.includes("--check");

const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
const baselinePackages = baseline.packages ?? {};

function listPackageDirs(scope) {
	const root = join(registryRoot, scope);
	return readdirSync(root, { withFileTypes: true })
		.filter((entry) => entry.isDirectory())
		.map((entry) => entry.name)
		.sort();
}

function classifyVitest(result) {
	const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
	if (result.status === 0) {
		if (/\bskipped\b/i.test(output) && !/\bpassed\b/i.test(output)) {
			return "disabled";
		}
		return "working";
	}

	if (
		/Transform failed|Failed to load|Cannot find module|SyntaxError|TSError|TypeError: Unknown file extension/i.test(
			output,
		)
	) {
		return "not-compiling";
	}

	return "failing";
}

function runPackageTests(testDir) {
	const relTestDir = relative(registryRoot, testDir);
	return spawnSync(process.execPath, [runVitest, relTestDir], {
		cwd: registryRoot,
		encoding: "utf8",
		env: {
			...process.env,
			AGENTOS_WASM_COMMANDS_DIR: join(repoRoot, "packages/runtime-core/commands"),
			AGENTOS_C_WASM_COMMANDS_DIR: join(registryRoot, "native/c/build"),
		},
	});
}

function currentStatus(scope, name, baselineEntry) {
	const testDir = join(registryRoot, scope, name, "test");
	if (!existsSync(testDir)) {
		return baselineEntry?.baselineStatus ?? "no-test";
	}

	const result = runPackageTests(testDir);
	return classifyVitest(result);
}

const rows = [];

for (const scope of ["software", "agent"]) {
	for (const name of listPackageDirs(scope)) {
		const key = `${scope}/${name}`;
		const baselineEntry = baselinePackages[key] ?? {};
		const status = currentStatus(scope, name, baselineEntry);
		rows.push({
			key,
			baselineStatus: baselineEntry.baselineStatus ?? "no-test",
			status,
			commands: baselineEntry.commands ?? [],
			notes: baselineEntry.notes,
		});
	}
}

const regressions = rows.filter((row) => {
	if (row.baselineStatus === "working") return row.status !== "working";
	return false;
});

const updated = {
	schemaVersion: 1,
	generatedAt: new Date().toISOString(),
	packages: Object.fromEntries(
		rows.map((row) => [
			row.key,
			{
				baselineStatus: row.baselineStatus,
				status: row.status,
				commands: row.commands,
				...(row.notes ? { notes: row.notes } : {}),
			},
		]),
	),
};

writeFileSync(baselinePath, `${JSON.stringify(updated, null, 2)}\n`);

const keyWidth = Math.max("package".length, ...rows.map((row) => row.key.length));
const baselineWidth = "baseline".length;
console.log(
	`${"package".padEnd(keyWidth)}  ${"baseline".padEnd(baselineWidth)}  status`,
);
console.log(`${"-".repeat(keyWidth)}  ${"-".repeat(baselineWidth)}  ------`);
for (const row of rows) {
	console.log(
		`${row.key.padEnd(keyWidth)}  ${row.baselineStatus.padEnd(baselineWidth)}  ${row.status}`,
	);
}

if (regressions.length > 0) {
	console.error("\nRegistry test regressions:");
	for (const row of regressions) {
		console.error(`- ${row.key}: baseline working, now ${row.status}`);
	}
	process.exit(1);
}

if (checkMode) {
	console.log("\nNo registry package test regressions detected.");
}
