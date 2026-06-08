import { describe, expect, test } from "vitest";

const PACKAGE_EXPORTS = [
	"../dist/index.js",
	"../dist/driver.js",
	"../dist/kernel-runtime.js",
] as const;

describe("python package exports", () => {
	test.each(PACKAGE_EXPORTS)("%s is importable after build", async (specifier) => {
		await expect(import(specifier)).resolves.toBeTypeOf("object");
	});
});
