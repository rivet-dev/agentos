import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { checkRegistryTestRuntimeBoundary } from "./check-registry-test-runtime-boundary.mjs";

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "registry-test-runtime-boundary-"));
	try {
		return fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function write(root, rel, contents) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, contents);
}

test("accepts the central registry test runtime helper", () => {
	withFixture((root) => {
		write(
			root,
			"registry/tests/helpers.ts",
			'export { createWasmVmRuntime } from "@secure-exec/core/test-runtime";\n',
		);
		write(
			root,
			"registry/tests/wasmvm/example.test.ts",
			'import { createWasmVmRuntime } from "../helpers.js";\n',
		);

		assert.deepEqual(checkRegistryTestRuntimeBoundary({ root }), []);
	});
});

test("rejects direct Agent OS test runtime imports in registry tests", () => {
	withFixture((root) => {
		write(
			root,
			"registry/tests/wasmvm/example.test.ts",
			'import { createWasmVmRuntime } from "@rivet-dev/agentos-core/test/runtime";\n',
		);

		assert.deepEqual(checkRegistryTestRuntimeBoundary({ root }), [
			"registry/tests/wasmvm/example.test.ts must import registry test runtime helpers from ../helpers.js instead of @rivet-dev/agentos-core/test/runtime",
		]);
	});
});

test("rejects direct re-exports and requires", () => {
	withFixture((root) => {
		write(
			root,
			"registry/tests/kernel/example.test.ts",
			'export { createKernel } from "@rivet-dev/agentos-core/test/runtime";\nconst rt = require("@rivet-dev/agentos-core/test/runtime");\n',
		);

		assert.deepEqual(checkRegistryTestRuntimeBoundary({ root }), [
			"registry/tests/kernel/example.test.ts must import registry test runtime helpers from ../helpers.js instead of @rivet-dev/agentos-core/test/runtime",
		]);
	});
});

test("rejects direct Agent OS runtime compat imports in registry tests", () => {
	withFixture((root) => {
		write(
			root,
			"registry/tests/wasmvm/example.test.ts",
			'import { createWasmVmRuntime } from "@rivet-dev/agentos-core/internal/runtime-compat";\n',
		);

		assert.deepEqual(checkRegistryTestRuntimeBoundary({ root }), [
			"registry/tests/wasmvm/example.test.ts must import registry test runtime helpers from ../helpers.js instead of @rivet-dev/agentos-core/internal/runtime-compat",
		]);
	});
});

test("rejects direct secure-exec test runtime imports in registry tests", () => {
	withFixture((root) => {
		write(
			root,
			"registry/tests/wasmvm/example.test.ts",
			'import { createWasmVmRuntime } from "@secure-exec/core/test-runtime";\n',
		);

		assert.deepEqual(checkRegistryTestRuntimeBoundary({ root }), [
			"registry/tests/wasmvm/example.test.ts must import registry test runtime helpers from ../helpers.js instead of @secure-exec/core/test-runtime",
		]);
	});
});
