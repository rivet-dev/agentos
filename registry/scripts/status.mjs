#!/usr/bin/env node

/**
 * Registry package status: local version, staged bin/, assembled dist/package,
 * and (with --remote) the published npm dist-tags per package.
 *
 * Run from anywhere: node registry/scripts/status.mjs [--remote]
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REGISTRY_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const remote = process.argv.includes("--remote");

function countEntries(dir) {
	try {
		return readdirSync(dir).length;
	} catch {
		return null;
	}
}

function distTags(name) {
	try {
		const out = execFileSync(
			"npm",
			["view", name, "dist-tags", "--json"],
			{ encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
		);
		const tags = JSON.parse(out);
		return Object.entries(tags)
			.map(([tag, version]) => `${tag}=${version}`)
			.join(" ");
	} catch {
		return "(not published)";
	}
}

const rows = [];
for (const kind of ["software", "agent"]) {
	const base = join(REGISTRY_ROOT, kind);
	for (const dir of readdirSync(base, { withFileTypes: true })) {
		if (!dir.isDirectory()) continue;
		const pkgPath = join(base, dir.name, "package.json");
		if (!existsSync(pkgPath)) continue;
		const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
		const bin = countEntries(join(base, dir.name, "bin"));
		const distPackage = join(base, dir.name, "dist", "package");
		const assembled = existsSync(join(distPackage, "package.json"))
			? (countEntries(join(distPackage, "bin")) ?? 0)
			: null;
		rows.push({
			kind,
			name: pkg.name,
			version: pkg.version,
			bin: bin === null ? "-" : String(bin),
			dist: assembled === null ? "NOT BUILT" : `${assembled} cmds`,
			tags: remote ? distTags(pkg.name) : undefined,
		});
	}
}

const width = Math.max(...rows.map((r) => r.name.length)) + 2;
for (const row of rows) {
	process.stdout.write(
		`${row.name.padEnd(width)} ${row.version.padEnd(28)} bin:${row.bin.padEnd(5)} dist:${row.dist.padEnd(10)}${row.tags !== undefined ? ` ${row.tags}` : ""}\n`,
	);
}
const nativeCommands = join(
	REGISTRY_ROOT,
	"native/target/wasm32-wasip1/release/commands",
);
const built = countEntries(nativeCommands);
process.stdout.write(
	`\nnative commands dir: ${built === null ? "NOT BUILT (run `just registry-native`)" : `${built} entries`}\n`,
);
