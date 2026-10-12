import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("Python startup diagnostics work before a guest process owns stdio", async () => {
	const vm = await AgentOs.create({ defaultSoftware: false });
	try {
		const result = await vm.process.execFile("python", ["-c", "print(42)"], {
			env: { AGENTOS_PYTHON_WARMUP_DEBUG: "1" },
			timeoutMs: 30_000,
			output: { capture: "all" },
		});
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("42\n");
		expect(result.stderr).toContain("__AGENTOS_PYTHON_WARMUP_METRICS__:");
	} finally {
		await vm.dispose();
	}
}, 60_000);
