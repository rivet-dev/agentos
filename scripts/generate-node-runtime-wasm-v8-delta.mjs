#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputPath = join(root, "docs-internal/node-runtime-wasm-v8-delta.json");
const nodeInclude = join(root, "crates/node-runtime-wasm/vendor/node/deps/v8/include");
const rustyInclude = join(root, "crates/node-runtime-wasm/vendor/rustyV8/v8/include");

function slash(path) {
	return path.split(sep).join("/");
}

function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

function collectFiles(path, base = path, output = {}) {
	for (const entry of readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
		const child = join(path, entry.name);
		if (entry.isDirectory()) collectFiles(child, base, output);
		else if (entry.isFile()) {
			const bytes = readFileSync(child);
			output[slash(relative(base, child))] = { bytes: bytes.length, sha256: sha256(bytes) };
		}
	}
	return output;
}

function compare(left, right) {
	const rows = {};
	const counts = { exact: 0, modified: 0, nodeOnly: 0, rustyV8Only: 0 };
	for (const path of [...new Set([...Object.keys(left), ...Object.keys(right)])].sort((a, b) => a.localeCompare(b))) {
		const node = left[path];
		const rustyV8 = right[path];
		let status;
		if (!rustyV8) status = "node-only";
		else if (!node) status = "rusty-v8-only";
		else if (node.sha256 === rustyV8.sha256) status = "exact";
		else status = "modified";
		const key = status.replace(/-([a-z0-9])/g, (_, value) => value.toUpperCase()).replace("RustyV8", "rustyV8");
		counts[key]++;
		rows[path] = { status, node: node ?? null, rustyV8: rustyV8 ?? null };
	}
	return { counts, rows };
}

function macro(path, name) {
	const source = readFileSync(path, "utf8");
	const match = source.match(new RegExp(`#define ${name} (\\d+)`));
	if (!match) throw new Error(`missing ${name} in ${slash(relative(root, path))}`);
	return Number(match[1]);
}

function v8Version(path) {
	return ["V8_MAJOR_VERSION", "V8_MINOR_VERSION", "V8_BUILD_NUMBER", "V8_PATCH_LEVEL"]
		.map((name) => macro(path, name))
		.join(".");
}

function icuVersion(path) {
	return `${macro(path, "U_ICU_VERSION_MAJOR_NUM")}.${macro(path, "U_ICU_VERSION_MINOR_NUM")}`;
}

function rustV8ApiUses() {
	const src = join(root, "crates/v8-runtime/src");
	const rows = {};
	for (const [path, identity] of Object.entries(collectFiles(src))) {
		if (!path.endsWith(".rs")) continue;
		const source = readFileSync(join(src, path), "utf8");
		for (const match of source.matchAll(/\bv8::([A-Za-z_][A-Za-z0-9_]*)/g)) {
			(rows[match[1]] ??= []).push(path);
		}
		void identity;
	}
	return Object.fromEntries(
		Object.entries(rows)
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([name, paths]) => [name, [...new Set(paths)].sort((a, b) => a.localeCompare(b))]),
	);
}

for (const path of [nodeInclude, rustyInclude]) {
	if (!existsSync(path)) throw new Error(`required vendored V8 headers are missing: ${slash(relative(root, path))}`);
}

const report = {
	schema: 1,
	decision: "reviewed-delta-required",
	node: {
		version: v8Version(join(nodeInclude, "v8-version.h")),
		fullVersion: "13.6.233.17-node.48",
		icuVersion: icuVersion(join(root, "crates/node-runtime-wasm/vendor/node/deps/icu-small/source/common/unicode/uvernum.h")),
		commit: "848430679556aed0bd073f2bc263331ad84fa119",
	},
	rustyV8: {
		crateVersion: "136.0.0",
		crateChecksum: "278d906d3513fce0be40e1b28eb4c482f44e9d3bf7c1be880441e706bebf5e43",
		vcs: "6a4f2ad9f4ee9677a421832679905b4bee1264f6",
		v8Version: v8Version(join(rustyInclude, "v8-version.h")),
		icuVersion: icuVersion(join(root, "crates/node-runtime-wasm/vendor/rustyV8/third_party/icu/source/common/unicode/uvernum.h")),
	},
	publicHeaderDelta: compare(collectFiles(nodeInclude), collectFiles(rustyInclude)),
	agentOsRustV8ApiUses: rustV8ApiUses(),
};

const rendered = `${JSON.stringify(report, null, 2)}\n`;
if (process.argv.includes("--check")) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== rendered) {
		throw new Error("node-runtime-wasm V8 delta is stale; regenerate it without --check");
	}
} else {
	writeFileSync(outputPath, rendered);
}
process.stdout.write(
	`v8-delta: ${process.argv.includes("--check") ? "verified" : "generated"}; ` +
		`${report.node.version}/${report.node.icuVersion} -> ${report.rustyV8.v8Version}/${report.rustyV8.icuVersion}; ` +
		`${JSON.stringify(report.publicHeaderDelta.counts)}\n`,
);
