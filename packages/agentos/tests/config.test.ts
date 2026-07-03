import { describe, expect, test } from "vitest";
import { agentOS } from "../src/index.js";

describe("@rivet-dev/agentos native actor config", () => {
	test("rejects create options that cannot cross the NAPI config boundary", () => {
		expect(() =>
			agentOS({
				createOptions: () => ({}),
			} as never),
		).toThrow(/createOptions/);
	});
});
