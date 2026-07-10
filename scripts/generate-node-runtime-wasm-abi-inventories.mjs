#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourcePath = resolve(repoRoot, "docs-internal/node-runtime-wasm-abi.json");
const outputDirectory = resolve(repoRoot, "docs-internal/node-runtime-wasm-abi");
const check = process.argv.slice(2).includes("--check");
if (process.argv.slice(2).some((argument) => argument !== "--check")) {
	throw new Error("usage: generate-node-runtime-wasm-abi-inventories.mjs [--check]");
}

const source = JSON.parse(readFileSync(sourcePath, "utf8"));
if (source.formatVersion !== 3) throw new Error("unsupported combined ABI manifest format");

const inventories = new Map([
	["agentos_napi_v1.json", { namespace: "agentos_napi_v1", entries: source.imports.filter((entry) => entry.module === "agentos_napi_v1") }],
	["agentos_node_engine_v1.json", { namespace: "agentos_node_engine_v1", entries: source.imports.filter((entry) => entry.module === "agentos_node_engine_v1") }],
	["agentos_posix_v1.json", { namespace: "agentos_posix_v1", entries: source.imports.filter((entry) => entry.module === "agentos_posix_v1") }],
	["reactor-exports.json", { namespace: "node-runtime-wasm-exports", entries: source.exports }],
]);

mkdirSync(outputDirectory, { recursive: true });
for (const [fileName, inventory] of inventories) {
	const output = `${JSON.stringify({
		schema: 1,
		source: "docs-internal/node-runtime-wasm-abi.json",
		wasmSha256: source.wasmSha256,
		...inventory,
	}, null, 2)}\n`;
	const outputPath = resolve(outputDirectory, fileName);
	if (check) {
		if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== output) {
			throw new Error(`generated ABI inventory is stale: ${outputPath}`);
		}
	} else {
		writeFileSync(outputPath, output);
	}
}

process.stdout.write(
	`Node runtime WASM ABI inventories ${check ? "verified" : "generated"}: ${[...inventories.values()].map(({ namespace, entries }) => `${namespace}=${entries.length}`).join(", ")}\n`,
);
