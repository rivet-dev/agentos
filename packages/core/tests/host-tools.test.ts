import { describe, expect, test } from "vitest";
import { z } from "zod";
import {
	HostToolSchemaConversionError,
	zodToJsonSchema,
} from "../src/host-tools-zod.js";
import {
	MAX_TOOL_DESCRIPTION_LENGTH,
	binding,
	bindingGroup,
	validateBindings,
} from "../src/index.js";

describe("host binding description limits", () => {
	test("accepts binding group and binding descriptions at the exported limit", () => {
		const description = "a".repeat(MAX_TOOL_DESCRIPTION_LENGTH);

		expect(() =>
			validateBindings([
				bindingGroup({
					name: "browser",
					description,
					bindings: {
						screenshot: binding({
							description,
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).not.toThrow();
	});

	test("rejects binding group descriptions longer than the exported limit", () => {
		expect(() =>
			validateBindings([
				bindingGroup({
					name: "browser",
					description: "a".repeat(MAX_TOOL_DESCRIPTION_LENGTH + 1),
					bindings: {
						screenshot: binding({
							description: "Take a screenshot",
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			`Binding group "browser" description is ${MAX_TOOL_DESCRIPTION_LENGTH + 1} characters, max is ${MAX_TOOL_DESCRIPTION_LENGTH}`,
		);
	});

	test("rejects binding descriptions longer than the exported limit", () => {
		expect(() =>
			validateBindings([
				bindingGroup({
					name: "browser",
					description: "Browser automation",
					bindings: {
						screenshot: binding({
							description: "a".repeat(MAX_TOOL_DESCRIPTION_LENGTH + 1),
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			`Binding "browser/screenshot" description is ${MAX_TOOL_DESCRIPTION_LENGTH + 1} characters, max is ${MAX_TOOL_DESCRIPTION_LENGTH}`,
		);
	});

	test("rejects binding group names that cannot become stable command names", () => {
		expect(() =>
			validateBindings([
				bindingGroup({
					name: "Browser_Tools",
					description: "Browser automation",
					bindings: {
						screenshot: binding({
							description: "Take a screenshot",
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			'Binding group name "Browser_Tools" must be lowercase alphanumeric with optional single hyphen separators',
		);
	});

	test("rejects binding names that cannot become stable subcommands", () => {
		expect(() =>
			validateBindings([
				bindingGroup({
					name: "browser-tools",
					description: "Browser automation",
					bindings: {
						"screenshot_now": binding({
							description: "Take a screenshot",
							inputSchema: z.object({ url: z.string() }),
							execute: () => ({ ok: true }),
						}),
					},
				}),
			]),
		).toThrow(
			'Binding name "screenshot_now" must be lowercase alphanumeric with optional single hyphen separators',
		);
	});

	test("fails loudly when a binding input schema uses an unsupported discriminated union", () => {
		const hostBinding = binding({
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
			zodToJsonSchema(hostBinding.inputSchema);
			throw new Error("Expected unsupported binding schema to fail");
		} catch (error) {
			expect(error).toBeInstanceOf(HostToolSchemaConversionError);
			expect(error).toMatchObject({
				path: "$.payload",
				zodType: "discriminatedUnion",
			});
		}
	});
});
