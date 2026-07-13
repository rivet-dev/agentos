import { describe, expect, test } from "vitest";
import { AgentOs, agentOsOptionsSchema } from "../src/index.js";

describe("AgentOsOptions validation", () => {
	test("rejects unknown top-level options before booting a VM", async () => {
		await expect(
			AgentOs.create({
				onSessionEvent: () => {},
			} as never),
		).rejects.toThrow(/onSessionEvent/);
	});

	test("rejects unknown nested permission fields", () => {
		expect(() =>
			agentOsOptionsSchema.parse({
				permissions: {
					filesystem: "allow",
				},
			}),
		).toThrow(/filesystem/);
	});

	test("rejects create option factories on the one-shot core constructor", () => {
		expect(() =>
			agentOsOptionsSchema.parse({
				createOptions: () => ({}),
			}),
		).toThrow(/createOptions/);
	});

	test("accepts toolKits as the public name for host tool groups", () => {
		expect(
			agentOsOptionsSchema.safeParse({
				toolKits: [
					{
						name: "weather",
						description: "Weather bindings",
						tools: {},
					},
				],
			}).success,
		).toBe(true);
	});

	test("preserves a positive VM aggregate captured-output budget", () => {
		const parsed = agentOsOptionsSchema.parse({
			limits: { resources: { maxCapturedOutputBytes: 2048 } },
		});
		expect(parsed.limits?.resources?.maxCapturedOutputBytes).toBe(2048);
		expect(
			agentOsOptionsSchema.safeParse({
				limits: { resources: { maxCapturedOutputBytes: 0 } },
			}).success,
		).toBe(false);
	});
});
