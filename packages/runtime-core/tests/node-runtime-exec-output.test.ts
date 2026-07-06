import { describe, expect, test } from "vitest";
import { NodeRuntime } from "../src/index.js";

describe("NodeRuntime execCommand output capture", () => {
	test(
		"captures complete stdout when a fast process exits immediately",
		async () => {
			const runtime = await NodeRuntime.create();
			const expected = "x".repeat(64 * 1024);
			const script = [
				'const fs = require("node:fs");',
				"const chunk = Buffer.alloc(4096, 120);",
				"for (let i = 0; i < 16; i += 1) fs.writeSync(1, chunk);",
				"process.exit(0);",
			].join(" ");

			try {
				for (let i = 0; i < 10; i += 1) {
					const result = await runtime.execCommand("node", ["-e", script]);

					expect(result.exitCode).toBe(0);
					expect(result.stdout).toBe(expected);
					expect(result.stderr).toBe("");
				}
			} finally {
				await runtime.dispose();
			}
		},
		120_000,
	);
});
