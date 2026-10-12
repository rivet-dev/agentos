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

for (const debugFirst of [true, false]) {
	test(`managed WASM diagnostics preserve ${debugFirst ? "fresh prewarm" : "cached prewarm"} and ordinary execution`, async () => {
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
			if (!debugFirst) {
				const plain = await vm.process.execFile("true", [], {
					timeoutMs: 5_000,
					output: { capture: "all", maxBufferedBytes: 64 * 1024 },
				});
				expect(plain.exitCode, plain.stderr).toBe(0);
				expect(plain.stdout).toBe("");
				expect(plain.stderr).toBe("");
			}
			for (let iteration = 0; iteration < 2; iteration++) {
				const result = await vm.process.execFile("true", [], {
					env: debugEnvironment,
					timeoutMs: 5_000,
					output: { capture: "all", maxBufferedBytes: 64 * 1024 },
				});
				expect(result.exitCode, result.stderr).toBe(0);
				expect(result.stdout).toBe("");
				const cold = debugFirst && iteration === 0;
				expect(
					metricRecords(result.stderr, "__AGENTOS_WASM_WARMUP_METRICS__:"),
				).toEqual([
					expect.objectContaining({
						executed: cold,
						reason: cold ? "executed" : "cached",
					}),
				]);
				const phases = metricRecords(
					result.stderr,
					"__AGENTOS_WASM_PHASE_METRICS__:",
				);
				expect(phases.some((record) => record.reason === "complete")).toBe(
					true,
				);
				expect(phases.some((record) => record.reason === "prewarm")).toBe(cold);
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
}
