import { describe, expect, test } from "vitest";
import { z } from "zod";
import {
	HostFunctionSchemaConversionError,
	zodToJsonSchema,
} from "../src/host-functions-zod.js";
import {
	MAX_HOST_FUNCTION_DESCRIPTION_LENGTH,
	hostFunction,
	hostFunctions,
	validateHostFunctions,
} from "../src/index.js";

describe("host-function description limits", () => {
	test("accepts collection and function descriptions at the exported limit", () => {
		const description = "a".repeat(MAX_HOST_FUNCTION_DESCRIPTION_LENGTH);

		expect(() =>
			validateHostFunctions([
				hostFunctions({
					name: "browser",
					description,
					functions: {
						screenshot: hostFunction({
							description,
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).not.toThrow();
	});

	test("rejects collection descriptions longer than the exported limit", () => {
		expect(() =>
			validateHostFunctions([
				hostFunctions({
					name: "browser",
					description: "a".repeat(MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1),
					functions: {
						screenshot: hostFunction({
							description: "Take a screenshot",
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			`Host function collection "browser" description is ${MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1} characters, max is ${MAX_HOST_FUNCTION_DESCRIPTION_LENGTH}`,
		);
	});

	test("rejects function descriptions longer than the exported limit", () => {
		expect(() =>
			validateHostFunctions([
				hostFunctions({
					name: "browser",
					description: "Browser automation",
					functions: {
						screenshot: hostFunction({
							description: "a".repeat(MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1),
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			`Host function "browser/screenshot" description is ${MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1} characters, max is ${MAX_HOST_FUNCTION_DESCRIPTION_LENGTH}`,
		);
	});

	test("rejects collection names that cannot become stable command names", () => {
		expect(() =>
			validateHostFunctions([
				hostFunctions({
					name: "Browser_Host_Functions",
					description: "Browser automation",
					functions: {
						screenshot: hostFunction({
							description: "Take a screenshot",
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			'Host function collection name "Browser_Host_Functions" must be lowercase alphanumeric with optional single hyphen separators',
		);
	});

	test("rejects function names that cannot become stable subcommands", () => {
		expect(() =>
			validateHostFunctions([
				hostFunctions({
					name: "browser-host-functions",
					description: "Browser automation",
					functions: {
						screenshot_now: hostFunction({
							description: "Take a screenshot",
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			'Host function name "screenshot_now" must be lowercase alphanumeric with optional single hyphen separators',
		);
	});

	test("fails loudly when a host-function input schema uses an unsupported discriminated union", () => {
		const definition = hostFunction({
			description: "Inspect a variant payload",
			inputSchema: z.object({
				payload: z.discriminatedUnion("kind", [
					z.object({ kind: z.literal("text"), value: z.string() }),
					z.object({ kind: z.literal("code"), status: z.number() }),
				]),
			}),
			execute: () => ({ ok: true }),
		});

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
