import { describe, expect, it } from "vitest";
import buildEssential, { common, git, ssh, curl } from "../src/index.js";

describe("build-essential meta-package", () => {
	it("includes Git and its SSH transport exactly once", () => {
		expect(buildEssential).toEqual([...common, git, ssh, curl]);
		expect(new Set(buildEssential).size).toBe(buildEssential.length);
		for (const descriptor of buildEssential) {
			expect(descriptor).toEqual({
				packagePath: expect.stringMatching(/\/package\.aospkg$/),
			});
		}
	});
});
