import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const packageDir = new URL("..", import.meta.url).pathname;

describe("package manifest", () => {
	it("declares command binaries", () => {
		const manifest = JSON.parse(
			readFileSync(join(packageDir, "agentos-package.json"), "utf8"),
		);

		expect(manifest.commands?.length ?? 0).toBeGreaterThan(0);
		for (const command of manifest.commands) {
			expect(typeof command).toBe("string");
			expect(command.length).toBeGreaterThan(0);
		}
	});
});
