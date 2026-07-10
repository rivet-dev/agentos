#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const policyPath = resolve(repoRoot, "docs-internal/node-runtime-wasm-forbidden.json");

function parseArgs(argv) {
	const result = { abi: "", build: "" };
	for (let index = 0; index < argv.length; index += 1) {
		const flag = argv[index];
		if (flag !== "--abi" && flag !== "--build") throw new Error(`unknown argument: ${flag}`);
		const value = argv[++index];
		if (!value) throw new Error(`${flag} requires a path`);
		result[flag.slice(2)] = resolve(value);
	}
	if (!result.abi || !result.build) {
		throw new Error("usage: check-node-runtime-wasm-forbidden.mjs --abi ABI.json --build BUILD.json");
	}
	return result;
}

function readJson(path) {
	if (!existsSync(path)) throw new Error(`required gate input is missing: ${path}`);
	return JSON.parse(readFileSync(path, "utf8"));
}

function sha256(path) {
	return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function assert(condition, message) {
	if (!condition) throw new Error(message);
}

const options = parseArgs(process.argv.slice(2));
const policy = readJson(policyPath);
const abi = readJson(options.abi);
const build = readJson(options.build);
const abiBaseline = readJson(resolve(repoRoot, policy.abiBaseline));

assert(policy.schema === 1, "unsupported forbidden-policy schema");
assert(abi.formatVersion === 3, "unsupported Node runtime ABI manifest format");
assert(abiBaseline.formatVersion === 3, "unsupported frozen Node runtime ABI format");
assert(build.schema === 1, "unsupported Node runtime build manifest schema");

const requiredModules = policy.requiredImportModules;
const actualModules = [...new Set(abi.imports.map(({ module }) => module))].sort();
const expectedModules = Object.keys(requiredModules).sort();
assert(
	JSON.stringify(actualModules) === JSON.stringify(expectedModules),
	`Node runtime import modules drifted: ${JSON.stringify(actualModules)}`,
);

const forbiddenModules = new Set(policy.forbiddenImportModules);
for (const entry of abi.imports) {
	assert(!forbiddenModules.has(entry.module), `forbidden ABI import ${entry.module}.${entry.name}`);
	assert(
		entry.authority === requiredModules[entry.module],
		`authority drift for ${entry.module}.${entry.name}: ${entry.authority}`,
	);
	assert(
		typeof entry.resultClassification === "string" && entry.resultClassification.length > 0,
		`missing result classification for ${entry.module}.${entry.name}`,
	);
	assert(
		entry.testId === `abi:import:${entry.module}:${entry.name}`,
		`unstable ABI test id for ${entry.module}.${entry.name}`,
	);
}

for (const entry of abi.exports) {
	assert(
		typeof entry.resultClassification === "string" && entry.resultClassification.length > 0,
		`missing result classification for export ${entry.name}`,
	);
	assert(entry.testId === `abi:export:${entry.name}`, `unstable ABI test id for export ${entry.name}`);
}

const frozenShape = ({ imports, exports, memories, tables }) => ({
	imports,
	exports,
	memories,
	tables,
});
assert(
	JSON.stringify(frozenShape(abi)) === JSON.stringify(frozenShape(abiBaseline)),
	`Node runtime ABI drifted from ${policy.abiBaseline}; increment the versioned namespace or update the reviewed R0 freeze`,
);

for (const [namespace, inventoryPath] of Object.entries(policy.abiInventories)) {
	const inventory = readJson(resolve(repoRoot, inventoryPath));
	assert(inventory.schema === 1, `unsupported ABI inventory schema: ${inventoryPath}`);
	assert(inventory.namespace === namespace, `ABI inventory namespace drift: ${inventoryPath}`);
	assert(inventory.wasmSha256 === abiBaseline.wasmSha256, `ABI inventory module hash drift: ${inventoryPath}`);
	const expectedEntries = namespace === "node-runtime-wasm-exports"
		? abiBaseline.exports
		: abiBaseline.imports.filter((entry) => entry.module === namespace);
	assert(
		JSON.stringify(inventory.entries) === JSON.stringify(expectedEntries),
		`ABI inventory entries are stale: ${inventoryPath}`,
	);
}

const nodeApiContract = readJson(resolve(repoRoot, policy.nodeApiStableContract));
assert(nodeApiContract.schema === 1, "unsupported stable Node-API contract schema");
assert(nodeApiContract.namespace === "agentos_napi_v1", "stable Node-API namespace drifted");
assert(nodeApiContract.entryCount === 155, `stable Node-API v1-v10 count drifted: ${nodeApiContract.entryCount}`);
for (const source of nodeApiContract.sourceHeaders) {
	assert(source.sha256 === sha256(resolve(repoRoot, source.path)), `Node-API source header drifted: ${source.path}`);
}
assert(
	nodeApiContract.compiler.sha256 === sha256(resolve(repoRoot, nodeApiContract.compiler.path)),
	"Node-API inventory compiler drifted",
);
const nodeApiByName = new Map(nodeApiContract.entries.map((entry) => [entry.name, entry]));
assert(nodeApiByName.size === nodeApiContract.entryCount, "stable Node-API inventory has duplicate names");
for (const entry of nodeApiContract.entries) {
	assert(entry.nodeApiVersion >= 1 && entry.nodeApiVersion <= 10, `invalid Node-API version for ${entry.name}`);
	assert(entry.testId === `napi:v${entry.nodeApiVersion}:${entry.name}`, `unstable Node-API test id for ${entry.name}`);
	assert(entry.status === "required-provider-surface", `Node-API surface is not required: ${entry.name}`);
}
const experimentalImports = [];
for (const imported of abi.imports.filter(({ module }) => module === "agentos_napi_v1")) {
	const contract = nodeApiByName.get(imported.name);
	if (contract) {
		assert(contract.wireSignature === imported.signature, `Node-API wire signature drift for ${imported.name}`);
	} else {
		experimentalImports.push(imported.name);
	}
}
assert(
	JSON.stringify(experimentalImports.sort()) === JSON.stringify([...policy.napiRuntimeExperimentalImports].sort()),
	`unreviewed experimental imports entered agentos_napi_v1: ${experimentalImports.join(", ")}`,
);

const posixContract = readJson(resolve(repoRoot, policy.posixContract));
const posixAbi = abiBaseline.imports.filter(({ module }) => module === "agentos_posix_v1");
assert(posixContract.schema === 1, "unsupported POSIX contract schema");
assert(posixContract.namespace === "agentos_posix_v1", "POSIX contract namespace drifted");
assert(posixContract.entryCount === posixAbi.length, "POSIX contract does not cover every live import");
assert(
	posixContract.sourceAbiSha256 === sha256(resolve(repoRoot, posixContract.sourceAbi)),
	"POSIX contract does not bind its generated ABI inventory",
);
for (let index = 0; index < posixAbi.length; index += 1) {
	const abiEntry = posixAbi[index];
	const contract = posixContract.entries[index];
	assert(contract.name === abiEntry.name, `POSIX contract ordering/name drift at ${abiEntry.name}`);
	assert(contract.signature === abiEntry.signature, `POSIX signature drift at ${abiEntry.name}`);
	assert(contract.resultClassification === abiEntry.resultClassification, `POSIX result drift at ${abiEntry.name}`);
	assert(contract.authoritativeReference.startsWith("https://man7.org/linux/man-pages/"), `missing Linux/POSIX citation for ${abiEntry.name}`);
	assert(contract.sourceDeclarations.length > 0, `missing sysroot declaration for ${abiEntry.name}`);
	assert(contract.authorizationRule.length > 0, `missing authorization rule for ${abiEntry.name}`);
	assert(contract.accountingClasses.length > 0, `missing accounting class for ${abiEntry.name}`);
	assert(contract.bounds.length > 0, `missing bound for ${abiEntry.name}`);
	assert(contract.testId === `posix:agentos_posix_v1:${abiEntry.name}`, `unstable POSIX test id for ${abiEntry.name}`);
	assert(contract.status === "required-shared-provider-surface", `POSIX import is not required: ${abiEntry.name}`);
}

const forbiddenPosixPatterns = policy.forbiddenPosixNamePatterns.map((pattern) => new RegExp(pattern, "i"));
for (const entry of abi.imports.filter(({ module }) => module === "agentos_posix_v1")) {
	assert(
		forbiddenPosixPatterns.every((pattern) => !pattern.test(entry.name)),
		`Node-shaped host service is forbidden in agentos_posix_v1: ${entry.name}`,
	);
}

const lock = readFileSync(resolve(repoRoot, "Cargo.lock"), "utf8");
const packageNames = [...lock.matchAll(/^name = "([^"]+)"$/gm)].map((match) => match[1]);
for (const name of packageNames) {
	assert(
		policy.forbiddenCargoPackagePrefixes.every(
			(prefix) => name !== prefix && !name.startsWith(`${prefix}-`),
		),
		`forbidden second-engine Cargo package in release graph: ${name}`,
	);
}

assert(
	build.architecture?.javascriptEngine === "existing-native-v8-isolate" &&
		build.architecture?.wasmEngine === "v8-webassembly-module-instance" &&
		build.architecture?.hostCapabilityImportModule === "agentos_posix_v1",
	"build manifest does not describe the approved single-V8 architecture",
);
assert(build.wasm?.sha256 === abi.wasmSha256, "ABI/build module hashes disagree");
assert(build.abiManifest?.sha256 === sha256(options.abi), "build manifest does not bind the ABI manifest");

const wasmPath = resolve(dirname(options.build), "node-runtime.wasm");
assert(existsSync(wasmPath), `built module is missing beside build manifest: ${wasmPath}`);
assert(build.wasm.sha256 === sha256(wasmPath), "built module hash does not match build manifest");

for (const entry of [...policy.temporaryFileOnlyAllowlist, ...policy.legacyMigrationPaths]) {
	assert(/^R[0-7]$/.test(entry.deleteBy), `temporary allowlist entry lacks a valid deleting milestone: ${entry.path}`);
	assert(entry.reason.length > 0, `temporary allowlist entry lacks a reason: ${entry.path}`);
}

process.stdout.write(
	`node-runtime-wasm forbidden gate: ${abi.imports.length} imports use only ${actualModules.join(", ")}; no second engine or Node-shaped host service is reachable\n`,
);
