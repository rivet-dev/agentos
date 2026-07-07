import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const registryRoot = dirname(fileURLToPath(new URL("../package.json", import.meta.url)));
const baselinePath = join(registryRoot, "test-status.json");
const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));

const trackedGapStatuses = new Set(["no-test", "not-buildable", "meta"]);
const packages = baseline.packages ?? {};

function packageDirs(scope) {
	const root = join(registryRoot, scope);
	return readdirSync(root, { withFileTypes: true })
		.filter((entry) => entry.isDirectory())
		.map((entry) => entry.name)
		.sort();
}

const failures = [];

for (const name of packageDirs("software")) {
	const key = `software/${name}`;
	const testDir = join(registryRoot, "software", name, "test");
	const entry = packages[key];

	if (existsSync(testDir)) continue;
	if (entry && trackedGapStatuses.has(entry.baselineStatus)) continue;

	failures.push(`${key} has no test/ directory and no tracked gap entry`);
}

for (const name of packageDirs("agent")) {
	const key = `agent/${name}`;
	const testDir = join(registryRoot, "agent", name, "test");
	const entry = packages[key];

	if (existsSync(testDir)) continue;
	if (entry?.baselineStatus === "no-test") continue;

	failures.push(`${key} has no test/ directory and no tracked gap entry`);
}

if (failures.length > 0) {
	console.error("Registry test coverage gate failed:");
	for (const failure of failures) {
		console.error(`- ${failure}`);
	}
	process.exit(1);
}

console.log("Registry test coverage gate passed.");
