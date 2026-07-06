import { describe, expect, test } from "vitest";
import { protocol } from "../src/index.js";

describe("@rivet-dev/agentos-runtime-core raw protocol", () => {
	test("exports generated ExtEnvelope codec", () => {
		expect(protocol.writeExtEnvelope).toBeTypeOf("function");
		expect(protocol.readExtEnvelope).toBeTypeOf("function");
	});
});
