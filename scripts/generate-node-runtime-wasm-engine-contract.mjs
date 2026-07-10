#!/usr/bin/env node
import {
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const abiPath = resolve(
	repoRoot,
	"docs-internal/node-runtime-wasm-abi/agentos_node_engine_v1.json",
);
const outputPath = resolve(
	repoRoot,
	"docs-internal/node-runtime-wasm-abi/agentos-node-engine-contract.json",
);
const edgeSourceRoot = resolve(repoRoot, "crates/node-runtime-wasm/vendor/edgejs/src");
const referenceRoots = [
	resolve(repoRoot, "crates/node-runtime-wasm/vendor/napi/v8/src"),
	resolve(repoRoot, "crates/node-runtime-wasm/vendor/napi/src"),
];
const check = process.argv.slice(2).includes("--check");
if (process.argv.slice(2).some((argument) => argument !== "--check")) {
	throw new Error("usage: generate-node-runtime-wasm-engine-contract.mjs [--check]");
}

function sourceFiles(root) {
	const files = [];
	for (const name of readdirSync(root)) {
		const path = resolve(root, name);
		if (statSync(path).isDirectory()) files.push(...sourceFiles(path));
		else if (/\.(?:c|cc|cpp|h|hpp|rs)$/.test(name)) files.push(path);
	}
	return files.sort();
}

function locations(files, symbol) {
	const hits = [];
	for (const path of files) {
		const lines = readFileSync(path, "utf8").split("\n");
		for (let index = 0; index < lines.length; index += 1) {
			if (lines[index].includes(symbol)) {
				hits.push(`${relative(repoRoot, path)}:${index + 1}`);
			}
		}
	}
	return hits;
}

function referenceDefinition(files, symbol) {
	for (const path of files) {
		const source = readFileSync(path, "utf8");
		const match = new RegExp(`\\b${symbol}\\s*\\(`).exec(source);
		if (!match) continue;
		const brace = source.indexOf("{", match.index + match[0].length);
		if (brace < 0) continue;
		let depth = 0;
		let end = brace;
		for (; end < source.length; end += 1) {
			if (source[end] === "{") depth += 1;
			else if (source[end] === "}") {
				depth -= 1;
				if (depth === 0) {
					end += 1;
					break;
				}
			}
		}
		const line = source.slice(0, match.index).split("\n").length;
		const body = source.slice(brace, end);
		const v8Apis = [
			...new Set(body.match(/\bv8::[A-Za-z_][A-Za-z0-9_:]*/g) ?? []),
		].sort();
		return {
			source: `${relative(repoRoot, path)}:${line}`,
			v8Apis,
		};
	}
	return null;
}

function capabilityFamily(name) {
	if (name.includes("contextify") || name.includes("bytecode")) return "context-and-compilation";
	if (name.includes("module_wrap")) return "es-modules";
	if (name.includes("promise")) return "promise-hooks-and-details";
	if (name.includes("microtask") || name.includes("foreground_task")) return "microtasks-and-foreground-tasks";
	if (name.includes("structured_clone") || name.includes("serializ") || name.includes("serdes")) return "structured-clone";
	if (name.includes("heap") || name.includes("memory") || name.includes("terminate") || name.includes("stack_limit") || name.includes("gc_for_testing")) return "heap-and-termination";
	if (name.includes("profile") || name.includes("snapshot")) return "diagnostics-and-profiling";
	if (name.includes("error") || name.includes("call_sites") || name.includes("constructor_name") || name.includes("proxy") || name.includes("preview_entries") || name.includes("private_symbol") || name.includes("arraybuffer_view")) return "v8-inspection";
	return "runtime-lifecycle-hooks";
}

const abi = JSON.parse(readFileSync(abiPath, "utf8"));
if (abi.schema !== 1 || abi.namespace !== "agentos_node_engine_v1") {
	throw new Error("unsupported agentos_node_engine_v1 ABI inventory");
}
const edgeFiles = sourceFiles(edgeSourceRoot);
const referenceFiles = referenceRoots.flatMap(sourceFiles);
const entries = abi.entries.map((entry) => {
	const callers = locations(edgeFiles, entry.name);
	const reference = referenceDefinition(referenceFiles, entry.name);
	if (callers.length === 0) throw new Error(`engine import has no pinned EdgeJS caller: ${entry.name}`);
	if (reference === null) throw new Error(`engine import has no pinned reference implementation: ${entry.name}`);
	return {
		name: entry.name,
		signature: entry.signature,
		resultClassification: entry.resultClassification,
		capabilityFamily: capabilityFamily(entry.name),
		authorization: "isolate-local-v8-only",
		hostOsEffectsAllowed: false,
		callers,
		referenceImplementation: reference.source,
		referenceV8Apis: reference.v8Apis,
		providerStatus: "required-unimplemented",
		testId: entry.testId,
	};
});

const output = `${JSON.stringify({
	schema: 1,
	namespace: "agentos_node_engine_v1",
	source: "docs-internal/node-runtime-wasm-abi/agentos_node_engine_v1.json",
	wasmSha256: abi.wasmSha256,
	contract: "V8 isolate-local engine operations only; filesystem, network, process, crypto, compression, protocol, and unrestricted host operations are forbidden.",
	entries,
}, null, 2)}\n`;

mkdirSync(dirname(outputPath), { recursive: true });
if (check) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== output) {
		throw new Error(`generated Node engine contract is stale: ${outputPath}`);
	}
} else {
	writeFileSync(outputPath, output);
}

process.stdout.write(
	`Node runtime WASM engine contract ${check ? "verified" : "generated"}: ${entries.length} imports\n`,
);
