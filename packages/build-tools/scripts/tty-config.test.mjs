import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { build } from "esbuild";

test("TTY detection retries after an early bootstrap fallback", async () => {
	const result = await build({
		bundle: true,
		format: "iife",
		stdin: {
			contents: `
				import { _resolveRuntimeTtyConfig } from "./bridge-src/builtins/tty-config.ts";
				globalThis.resolveRuntimeTtyConfig = _resolveRuntimeTtyConfig;
			`,
			resolveDir: new URL("../", import.meta.url).pathname,
		},
		write: false,
	});
	const context = vm.createContext({});
	vm.runInContext(result.outputFiles[0].text, context);
	const resolve = () =>
		JSON.parse(
			vm.runInContext("JSON.stringify(resolveRuntimeTtyConfig())", context),
		);

	assert.deepEqual(
		resolve(),
		{
			stdinIsTTY: false,
			stdoutIsTTY: false,
			stderrIsTTY: false,
			cols: 80,
			rows: 24,
		},
	);

	context._kernelIsattyRaw = {
		applySync(_receiver, [fd]) {
			return fd === 0 || fd === 1 || fd === 2;
		},
	};
	context._kernelTtySizeRaw = {
		applySync() {
			return { cols: 100, rows: 32 };
		},
	};

	assert.deepEqual(
		resolve(),
		{
			stdinIsTTY: true,
			stdoutIsTTY: true,
			stderrIsTTY: true,
			cols: 100,
			rows: 32,
		},
	);
});
