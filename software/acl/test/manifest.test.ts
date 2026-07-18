import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const packageDir = new URL("..", import.meta.url).pathname;

describe("package manifest", () => {
	it("declares the POSIX ACL commands", () => {
		const manifest = JSON.parse(
			readFileSync(join(packageDir, "agentos-package.json"), "utf8"),
		);

		expect(manifest.commands).toEqual(["chacl", "getfacl", "setfacl"]);
	});
});
