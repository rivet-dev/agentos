import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Minimal WASM module exporting a no-op _start function.
const IMAGE = Uint8Array.from([
	0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00,
	0x00, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0a, 0x01, 0x06, 0x5f, 0x73, 0x74, 0x61,
	0x72, 0x74, 0x00, 0x00, 0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
]);

describe("WASM executable paths", () => {
	test("executes a written module by path after its execute bit is set", async () => {
		const vm = await AgentOs.create({ defaultSoftware: false });
		try {
			await vm.writeFile("/tmp/guest-tool", IMAGE);
			const denied = await vm.process.execFile("/tmp/guest-tool", [], {
				timeoutMs: 5_000,
				output: { capture: "all" },
			});
			expect(denied.error?.message).toContain("EACCES");
			const mode = await vm.process.execFile(
				"node",
				["-e", "require('node:fs').chmodSync('/tmp/guest-tool', 0o755)"],
				{
					timeoutMs: 5_000,
					output: { capture: "all" },
				},
			);
			expect(mode.exitCode).toBe(0);
			const executed = await vm.process.execFile("/tmp/guest-tool", [], {
				timeoutMs: 5_000,
				output: { capture: "all" },
			});
			expect(executed.exitCode).toBe(0);
			expect(executed.stdout).toBe("");
			expect(executed.stderr).toBe("");
		} finally {
			await vm.dispose();
		}
	}, 20_000);
});
