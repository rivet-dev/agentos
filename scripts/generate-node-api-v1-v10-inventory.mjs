#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const includeDirectory = resolve(repoRoot, "crates/node-runtime-wasm/vendor/napi/include");
const clang = resolve(repoRoot, "registry/native/c/vendor/wasi-sdk/bin/clang");
const sysroot = resolve(
	process.env.AGENTOS_NODE_API_SYSROOT ??
		"/home/nathan/progress/node-stdlib/2026-07-10-node-wasm-refactor-completion/sysroot-node-threads-final",
);
const outputPath = resolve(
	repoRoot,
	"docs-internal/node-runtime-wasm-abi/node-api-v1-v10.json",
);
const check = process.argv.slice(2).includes("--check");
if (process.argv.slice(2).some((argument) => argument !== "--check")) {
	throw new Error("usage: generate-node-api-v1-v10-inventory.mjs [--check]");
}

function sha256(path) {
	return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function collectFunctionDeclarations(node, output = []) {
	if (
		node?.kind === "FunctionDecl" &&
		(typeof node.name === "string") &&
		(node.name.startsWith("napi_") || node.name.startsWith("node_api_"))
	) {
		output.push(node);
	}
	for (const child of node?.inner ?? []) collectFunctionDeclarations(child, output);
	return output;
}

const i32Types = new Set([
	"_Bool",
	"int",
	"int32_t",
	"uint32_t",
	"size_t",
	"napi_key_collection_mode",
	"napi_key_conversion",
	"napi_key_filter",
	"napi_threadsafe_function_call_mode",
	"napi_threadsafe_function_release_mode",
	"napi_typedarray_type",
]);

function wireType(cType) {
	if (cType === "double") return "f64";
	if (cType === "int64_t" || cType === "uint64_t") return "i64";
	if (i32Types.has(cType) || cType.includes("*") || cType.startsWith("napi_") || cType === "node_api_basic_env" || cType === "node_api_basic_finalize") {
		return "i32";
	}
	throw new Error(`unclassified Node-API C wire type: ${cType}`);
}

function wireSignature(declaration) {
	const parameters = (declaration.inner ?? [])
		.filter(({ kind }) => kind === "ParmVarDecl")
		.map(({ type }) => wireType(type.qualType));
	const resultType = declaration.type.qualType.slice(0, declaration.type.qualType.indexOf(" (")).trim();
	const result = resultType === "void" ? [] : [wireType(resultType)];
	return `(func${parameters.length > 0 ? ` (param ${parameters.join(" ")})` : ""}${result.length > 0 ? ` (result ${result.join(" ")})` : ""})`;
}

const handleTypes = /^(node_api_basic_env|napi_(env|value|ref|deferred|async_context|async_work|callback_info|callback_scope|escapable_handle_scope|handle_scope|threadsafe_function|async_cleanup_hook_handle))$/;
const callbackTypes = /^(node_api_basic_finalize|napi_(callback|finalize|cleanup_hook|async_cleanup_hook|async_execute_callback|async_complete_callback|threadsafe_function_call_js))$/;

function parameterRole(cType) {
	if (handleTypes.test(cType)) return "handle";
	if (callbackTypes.test(cType)) return "guest-function-pointer";
	if (cType.includes("*")) {
		if (/^(const )?(napi_value|napi_ref|napi_deferred|napi_async_context|napi_async_work|napi_callback_scope|napi_escapable_handle_scope|napi_handle_scope|napi_threadsafe_function|napi_async_cleanup_hook_handle) \*$/.test(cType)) {
			return "guest-pointer-to-handle";
		}
		return cType.startsWith("const ") ? "guest-read-pointer" : "guest-write-pointer";
	}
	return "scalar";
}

function threadRequirement(name) {
	return name === "napi_call_threadsafe_function"
		? "documented-any-thread-entry"
		: "root-isolate-thread";
}

function lifecycleRequirement(name) {
	if (name.includes("threadsafe_function")) return "threadsafe-function-lifecycle";
	if (name.includes("async_work") || name.includes("async_context") || name.includes("callback_scope")) return "async-lifecycle";
	if (name.includes("cleanup_hook")) return "environment-cleanup";
	if (name.includes("reference") || name.endsWith("_ref") || name.endsWith("_unref")) return "reference-lifecycle";
	if (name.includes("handle_scope")) return "handle-scope-lifecycle";
	if (name.includes("wrap") || name.includes("finalizer") || name.includes("external")) return "finalizer-lifecycle";
	return "environment-live";
}

const versions = new Map();
for (let version = 1; version <= 10; version += 1) {
	const ast = JSON.parse(execFileSync(clang, [
		"--target=wasm32-wasi-threads",
		`--sysroot=${sysroot}`,
		`-DNAPI_VERSION=${version}`,
		`-I${includeDirectory}`,
		"-x", "c",
		"-Xclang", "-ast-dump=json",
		"-fsyntax-only",
		"-include", "node_api.h",
		"/dev/null",
	], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 }));
	const declarations = collectFunctionDeclarations(ast);
	const byName = new Map(declarations.map((declaration) => [declaration.name, declaration]));
	versions.set(version, byName);
}

const latest = versions.get(10);
const entries = [...latest.values()].map((declaration) => {
	const introduced = [...versions.entries()].find(([, declarations]) => declarations.has(declaration.name))?.[0];
	if (introduced === undefined) throw new Error(`could not determine version for ${declaration.name}`);
	for (let version = introduced; version <= 10; version += 1) {
		const candidate = versions.get(version).get(declaration.name);
		if (!candidate) throw new Error(`${declaration.name} disappeared in Node-API v${version}`);
		if (candidate.type.qualType !== declaration.type.qualType) {
			throw new Error(`${declaration.name} changed C signature in Node-API v${version}`);
		}
	}
	const parameters = (declaration.inner ?? [])
		.filter(({ kind }) => kind === "ParmVarDecl")
		.map(({ name, type }, index) => ({
			index,
			name: name || `arg${index}`,
			cType: type.qualType,
			wireType: wireType(type.qualType),
			role: parameterRole(type.qualType),
		}));
	return {
		name: declaration.name,
		nodeApiVersion: introduced,
		cSignature: declaration.type.qualType,
		wireSignature: wireSignature(declaration),
		resultClassification: declaration.type.qualType.startsWith("void (") ? "void" : "napi-status",
		threadRequirement: threadRequirement(declaration.name),
		lifecycleRequirement: lifecycleRequirement(declaration.name),
		parameters,
		testId: `napi:v${introduced}:${declaration.name}`,
		status: "required-provider-surface",
	};
}).sort((left, right) => left.name.localeCompare(right.name));

const output = `${JSON.stringify({
	schema: 1,
	namespace: "agentos_napi_v1",
	contract: "stable Node-API v1-v10",
	authoritativeDocumentation: "https://nodejs.org/docs/latest-v24.x/api/n-api.html",
	sourceHeaders: [
		{ path: "crates/node-runtime-wasm/vendor/napi/include/js_native_api.h", sha256: sha256(resolve(includeDirectory, "js_native_api.h")) },
		{ path: "crates/node-runtime-wasm/vendor/napi/include/node_api.h", sha256: sha256(resolve(includeDirectory, "node_api.h")) },
	],
	generator: "scripts/generate-node-api-v1-v10-inventory.mjs",
	compiler: { path: "registry/native/c/vendor/wasi-sdk/bin/clang", sha256: sha256(clang), target: "wasm32-wasi-threads" },
	entryCount: entries.length,
	entries,
}, null, 2)}\n`;

if (check) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== output) {
		throw new Error(`generated Node-API v1-v10 inventory is stale: ${outputPath}`);
	}
} else {
	writeFileSync(outputPath, output);
}
process.stdout.write(`Node-API v1-v10 inventory ${check ? "verified" : "generated"}: ${entries.length} stable functions\n`);
