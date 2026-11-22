import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const packagePath = fileURLToPath(
	new URL("../../../software/coreutils/dist/package.aospkg", import.meta.url),
);
const debugEnvironment = {
	AGENTOS_WASM_WARMUP_DEBUG: "1",
	AGENTOS_WASI_SYSCALL_PHASES: "1",
};

function metricRecords(
	stderr: string,
	prefix: string,
): Record<string, unknown>[] {
	return stderr
		.split("\n")
		.filter((line) => line.startsWith(prefix))
		.map((line) => JSON.parse(line.slice(prefix.length)));
}

test("managed WASM diagnostics appear on stderr only when requested", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		software: [{ packagePath }],
		permissions: {
			fs: "allow",
			childProcess: "allow",
			process: "allow",
			env: "allow",
			network: "deny",
		},
	});
	try {
		const plain = await vm.process.execFile("true", [], {
			timeoutMs: 5_000,
			output: { capture: "all", maxBufferedBytes: 64 * 1024 },
		});
		expect(plain.exitCode, plain.stderr).toBe(0);
		expect(plain.stdout).toBe("");
		expect(plain.stderr).toBe("");

		// The second debug run reuses the module admitted by the first.
		for (let iteration = 0; iteration < 2; iteration++) {
			const result = await vm.process.execFile("true", [], {
				env: debugEnvironment,
				timeoutMs: 5_000,
				output: { capture: "all", maxBufferedBytes: 64 * 1024 },
			});
			expect(result.exitCode, result.stderr).toBe(0);
			expect(result.stdout).toBe("");
			const phases = metricRecords(
				result.stderr,
				"__AGENTOS_WASM_PHASE_METRICS__:",
			);
			expect(phases).toEqual([
				expect.objectContaining({ reason: "completed", backend: "v8" }),
			]);
			if (iteration > 0) {
				expect(phases[0]?.moduleCacheHit).toBe(true);
			}
		}

		const ordinary = await vm.process.execFile("true", [], {
			timeoutMs: 5_000,
			output: { capture: "all", maxBufferedBytes: 64 * 1024 },
		});
		expect(ordinary.exitCode, ordinary.stderr).toBe(0);
		expect(ordinary.stdout).toBe("");
		expect(ordinary.stderr).toBe("");
	} finally {
		await vm.dispose();
	}
}, 20_000);
