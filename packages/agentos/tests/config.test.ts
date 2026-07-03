import { describe, expect, test } from "vitest";
import { agentOS } from "../src/index.js";

describe("@rivet-dev/agentos native actor config", () => {
	test("create options use the JS actor path instead of the native config boundary", () => {
		const definition = agentOS({
			createOptions: () => ({ defaultSoftware: false }),
		});

		expect(definition.nativeFactoryBuilder).toBeUndefined();
	});

	test("top-level sandbox provider uses the JS actor path", () => {
		const definition = agentOS({
			sandbox: {
				provider: {
					start: async () =>
						({
							baseUrl: "http://127.0.0.1:1234",
						}) as never,
				},
			},
		});

		expect(definition.nativeFactoryBuilder).toBeUndefined();
	});

	test("top-level sandbox client is rejected for actor instances", () => {
		expect(() =>
			agentOS({
				sandbox: {
					client: {
						baseUrl: "http://127.0.0.1:1234",
					} as never,
				},
			} as never),
		).toThrow(/sandbox: \{ client \}/);

		const definition = agentOS({
			createOptions: () => ({
				sandbox: {
					client: {
						baseUrl: "http://127.0.0.1:1234",
					} as never,
				},
			}),
		});
		expect(definition.nativeFactoryBuilder).toBeUndefined();
	});
});
