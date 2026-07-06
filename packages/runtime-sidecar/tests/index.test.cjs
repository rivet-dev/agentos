"use strict";

const assert = require("node:assert/strict");
const { mkdtempSync, rmSync, writeFileSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const test = require("node:test");
const { getSidecarPath } = require("../index.js");

const originalOverride = process.env.AGENTOS_SIDECAR_BIN;

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

	assert.throws(
		() => getSidecarPath(),
		/@rivet-dev\/agentos-runtime-sidecar: platform package .* is not installed/,
	);
});
