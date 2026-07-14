#!/usr/bin/env node

import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const execFileAsync = promisify(execFile);
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(scriptDir, "..", "..", "..");
const bridgeBuildScript = path.join(scriptDir, "build-v8-bridge.mjs");
const outputPath = path.join(
	workspaceRoot,
	"packages",
	"runtime-browser",
	"src",
	"generated",
	"native-v8-bridge.ts",
);

const temporaryDirectory = await mkdtemp(
	path.join(tmpdir(), "agentos-browser-native-bridge-"),
);

try {
	await execFileAsync(process.execPath, [
		bridgeBuildScript,
		"--out-dir",
		temporaryDirectory,
	]);
	const bridge = await readFile(
		path.join(temporaryDirectory, "v8-bridge.js"),
		"utf8",
	);
	const zlibBridge = await readFile(
		path.join(temporaryDirectory, "v8-bridge-zlib.js"),
		"utf8",
	);
	const source = [
		"// @generated - run pnpm --dir packages/runtime-browser generate:native-bridge",
		"// This is the same bridge payload embedded by the native AgentOS V8 runtime.",
		`export const NATIVE_V8_BRIDGE_CODE = ${JSON.stringify(`${bridge}\n${zlibBridge}`)};`,
		"",
	].join("\n");
	await writeFile(outputPath, source);
	console.log(
		`Built ${path.relative(workspaceRoot, outputPath)} (${source.length} bytes)`,
	);
} finally {
	await rm(temporaryDirectory, { force: true, recursive: true });
}
