"use strict";

const assert = require("node:assert/strict");
const { mkdtempSync, rmSync, writeFileSync } = require("node:fs");
const Module = require("node:module");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const test = require("node:test");
const { getSidecarPath } = require("../index.js");

const originalOverride = process.env.AGENTOS_SIDECAR_BIN;

function withPlatformPackage(packageJsonPath, run) {
	const originalResolve = Module._resolveFilename;
	Module._resolveFilename = function (request, parent, isMain, options) {
		if (
			request.startsWith("@rivet-dev/agentos-runtime-sidecar-") &&
			request.endsWith("/package.json")
		) {
			if (packageJsonPath === null) {
				const error = new Error(`Cannot find module '${request}'`);
				error.code = "MODULE_NOT_FOUND";
				throw error;
			}
			return packageJsonPath;
		}
		return originalResolve.call(this, request, parent, isMain, options);
	};
	try {
		return run();
	} finally {
		Module._resolveFilename = originalResolve;
	}
}

test.afterEach(() => {
	if (originalOverride === undefined) {
		delete process.env.AGENTOS_SIDECAR_BIN;
	} else {
		process.env.AGENTOS_SIDECAR_BIN = originalOverride;
	}
});

test("honors AGENTOS_SIDECAR_BIN when the file exists", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-native-sidecar-bin-"));
	try {
		const binaryPath = join(root, "agentos-native-sidecar");
		writeFileSync(binaryPath, "#!/bin/sh\n", { mode: 0o755 });
		process.env.AGENTOS_SIDECAR_BIN = binaryPath;

		assert.equal(getSidecarPath(), binaryPath);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("rejects a missing AGENTOS_SIDECAR_BIN override", () => {
	process.env.AGENTOS_SIDECAR_BIN = join(
		tmpdir(),
		`agentos-native-sidecar-missing-${process.pid}-${Date.now()}`,
	);

	assert.throws(
		() => getSidecarPath(),
		/AGENTOS_SIDECAR_BIN is set to .* but the file does not exist/,
	);
});

test("reports missing platform packages without chmod fallbacks", () => {
	delete process.env.AGENTOS_SIDECAR_BIN;

	withPlatformPackage(null, () =>
		assert.throws(
			() => getSidecarPath(),
			/@rivet-dev\/agentos-runtime-sidecar: platform package .* is not installed/,
		),
	);
});

test("pins the missing-package repair command to the resolver version", () => {
	delete process.env.AGENTOS_SIDECAR_BIN;
	withPlatformPackage(null, () =>
		assert.throws(() => getSidecarPath(), (error) => {
			const command = error.message.match(/Try: (.+)\n/)[1];
			assert.match(
				command,
				/^npm install --include=optional @rivet-dev\/agentos-runtime-sidecar-/,
			);
			assert.ok(command.endsWith(`@${require("../package.json").version}`));
			return true;
		}),
	);
});

test("rejects a stale platform binary version", () => {
	delete process.env.AGENTOS_SIDECAR_BIN;
	const root = mkdtempSync(join(tmpdir(), "agentos-stale-sidecar-"));
	try {
		const packageJsonPath = join(root, "package.json");
		writeFileSync(packageJsonPath, JSON.stringify({ version: "999.0.0" }));
		withPlatformPackage(packageJsonPath, () =>
			assert.throws(
				() => getSidecarPath(),
				(error) => error.message.includes(
					`has version 999.0.0; expected ${require("../package.json").version}`,
				),
			),
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("rejects a platform package with no binary", () => {
	delete process.env.AGENTOS_SIDECAR_BIN;
	const root = mkdtempSync(join(tmpdir(), "agentos-missing-sidecar-bin-"));
	try {
		const packageJsonPath = join(root, "package.json");
		writeFileSync(
			packageJsonPath,
			JSON.stringify({ version: require("../package.json").version }),
		);
		withPlatformPackage(packageJsonPath, () =>
			assert.throws(
				() => getSidecarPath(),
				/platform package .* is missing agentos-native-sidecar/,
			),
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("resolves a matching platform package binary", () => {
	delete process.env.AGENTOS_SIDECAR_BIN;
	const root = mkdtempSync(join(tmpdir(), "agentos-matching-sidecar-"));
	try {
		const packageJsonPath = join(root, "package.json");
		const binaryPath = join(root, "agentos-native-sidecar");
		writeFileSync(
			packageJsonPath,
			JSON.stringify({ version: require("../package.json").version }),
		);
		writeFileSync(binaryPath, "#!/bin/sh\n", { mode: 0o755 });
		withPlatformPackage(packageJsonPath, () =>
			assert.equal(getSidecarPath(), binaryPath),
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
