import {
	HostFunctionSchemaConversionError,
	zodToJsonSchema,
} from "../src/host-functions-zod.js";
import { describe, expect, test } from "vitest";
import { z } from "zod";
import {
	hostFunctionCommandName,
	hostFunctionDescription,
	resolveHostFunctions,
} from "../src/index.js";

const screenshot = {
	inputSchema: z.object({ url: z.string() }).describe("Take a screenshot"),
	execute: () => ({ ok: true }),
};

describe("host-function names", () => {
	test("converts camelCase keys to kebab-case command names", () => {
		expect(hostFunctionCommandName("orderStore")).toBe("order-store");
		expect(hostFunctionCommandName("listOpenOrders")).toBe("list-open-orders");
		expect(hostFunctionCommandName("order-store")).toBe("order-store");
		expect(hostFunctionCommandName("orders")).toBe("orders");
	});

	test("resolves both key spellings to the same command names", () => {
		expect(
			resolveHostFunctions({ orderStore: { listOrders: screenshot } }),
		).toEqual([
			{ name: "order-store", functions: { "list-orders": screenshot } },
		]);
	});

	test("rejects collection keys that cannot become command names", () => {
		expect(() =>
			resolveHostFunctions({ Browser_Host_Functions: { screenshot } }),
		).toThrow(
			'Host function collection name "Browser_Host_Functions" must be alphanumeric, written in camelCase or with single hyphen separators',
		);
	});

	test("rejects function keys that cannot become subcommands", () => {
		expect(() =>
			resolveHostFunctions({ browser: { screenshot_now: screenshot } }),
		).toThrow(
			'Host function name "screenshot_now" must be alphanumeric, written in camelCase or with single hyphen separators',
		);
	});

	test("rejects two collection keys that resolve to the same command name", () => {
		expect(() =>
			resolveHostFunctions({
				orderStore: { screenshot },
				"order-store": { screenshot },
			}),
		).toThrow(
			'Host function collections "orderStore" and "order-store" both resolve to the command name "order-store"',
		);
	});

	test("rejects two function keys that resolve to the same command name", () => {
		expect(() =>
			resolveHostFunctions({
				browser: { screenshotNow: screenshot, "screenshot-now": screenshot },
			}),
		).toThrow(
			'Host functions "screenshotNow" and "screenshot-now" in collection "browser" both resolve to the command name "screenshot-now"',
		);
	});
});

describe("host-function descriptions", () => {
	test("reads the description from the input schema", () => {
		expect(hostFunctionDescription(screenshot)).toBe("Take a screenshot");
	});

	test("is empty when the schema carries no description", () => {
		expect(
			hostFunctionDescription({
				inputSchema: z.object({ url: z.string() }),
				execute: () => ({ ok: true }),
			}),
		).toBe("");
	});
});

describe("host-function schemas", () => {
	test("fails loudly when a host-function input schema uses an unsupported discriminated union", () => {
		const definition = {
			inputSchema: z
				.object({
					payload: z.discriminatedUnion("kind", [
						z.object({ kind: z.literal("text"), value: z.string() }),
						z.object({ kind: z.literal("code"), status: z.number() }),
					]),
				})
				.describe("Inspect a variant payload"),
			execute: () => ({ ok: true }),
		};

		try {
			zodToJsonSchema(definition.inputSchema);
			throw new Error("Expected unsupported host-function schema to fail");
		} catch (error) {
			expect(error).toBeInstanceOf(HostFunctionSchemaConversionError);
			expect(error).toMatchObject({
				path: "$.payload",
				zodType: "discriminatedUnion",
			});
		}
	});
});
