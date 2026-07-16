import { describe, expect, test } from "vitest";
import { z } from "zod";
import { z as z3 } from "zod3";
import {
	BindingSchemaConversionError,
	zodToJsonSchema,
} from "../src/bindings-zod.js";

describe("zodToJsonSchema", () => {
	test("converts objects with supported scalar constraints", () => {
		const schema = z.object({
			url: z
				.string()
				.min(1)
				.max(128)
				.regex(/^https?:\/\//)
				.describe("Target URL"),
			fullPage: z.boolean().optional(),
			format: z.enum(["png", "jpg"]).describe("Image format"),
			width: z.number().min(320).max(1920).optional(),
		});

		expect(zodToJsonSchema(schema)).toEqual({
			type: "object",
			properties: {
				url: {
					type: "string",
					minLength: 1,
					maxLength: 128,
					pattern: "^https?:\\/\\/",
					description: "Target URL",
				},
				fullPage: { type: "boolean" },
				format: {
					type: "string",
					enum: ["png", "jpg"],
					description: "Image format",
				},
				width: {
					type: "number",
					minimum: 320,
					maximum: 1920,
				},
			},
			required: ["url", "format"],
		});
	});

	test("converts nested objects, arrays, unions, literals, and nullable fields", () => {
		const schema = z.object({
			tags: z.array(z.string()),
			options: z.object({
				mode: z.union([z.literal("fast"), z.literal("safe")]),
				note: z.string().nullable(),
			}),
			env: z.record(z.string(), z.string()).optional(),
		});

		expect(zodToJsonSchema(schema)).toEqual({
			type: "object",
			properties: {
				tags: {
					type: "array",
					items: { type: "string" },
				},
				options: {
					type: "object",
					properties: {
						mode: {
							anyOf: [
								{ type: "string", const: "fast" },
								{ type: "string", const: "safe" },
							],
						},
						note: {
							anyOf: [{ type: "string" }, { type: "null" }],
						},
					},
					required: ["mode", "note"],
				},
				env: {
					type: "object",
					propertyNames: { type: "string" },
					additionalProperties: { type: "string" },
				},
			},
			required: ["tags", "options"],
		});
	});

	test("converts equivalent Zod v3 schemas", () => {
		const schema = z3.object({
			url: z3
				.string()
				.min(1)
				.max(128)
				.regex(/^https?:\/\//)
				.describe("Target URL"),
			fullPage: z3.boolean().optional(),
			format: z3.enum(["png", "jpg"]).describe("Image format"),
			width: z3.number().min(320).max(1920).optional(),
			tags: z3.array(z3.string()),
			mode: z3.union([z3.literal("fast"), z3.literal("safe")]),
			env: z3.record(z3.string(), z3.string()).optional(),
		});

		expect(zodToJsonSchema(schema)).toEqual({
			type: "object",
			properties: {
				url: {
					type: "string",
					minLength: 1,
					maxLength: 128,
					pattern: "^https?:\\/\\/",
					description: "Target URL",
				},
				fullPage: { type: "boolean" },
				format: {
					type: "string",
					enum: ["png", "jpg"],
					description: "Image format",
				},
				width: {
					type: "number",
					minimum: 320,
					maximum: 1920,
				},
				tags: {
					type: "array",
					items: { type: "string" },
				},
				mode: {
					type: "string",
					enum: ["fast", "safe"],
				},
				env: {
					type: "object",
					additionalProperties: { type: "string" },
				},
			},
			required: ["url", "format", "tags", "mode"],
		});
	});

	test("throws a typed error for unsupported discriminated unions with the offending path", () => {
		const schema = z.object({
			payload: z.discriminatedUnion("kind", [
				z.object({ kind: z.literal("open"), value: z.string() }),
				z.object({ kind: z.literal("closed"), code: z.number() }),
			]),
		});

		expect(() => zodToJsonSchema(schema)).toThrowError(
			BindingSchemaConversionError,
		);
		expect(() => zodToJsonSchema(schema)).toThrowError(
			/Unsupported Zod schema at \$\.payload: discriminatedUnion/,
		);
	});

	test("rejects other lossy Zod constructs instead of silently coercing them", () => {
		const cases = [
			{
				path: "$.value",
				type: "record",
				schema: z.object({
					value: z.record(z.string().min(1), z.string()),
				}),
			},
			{
				path: "$.value",
				type: "tuple",
				schema: z.object({ value: z.tuple([z.string(), z.number()]) }),
			},
			{
				path: "$.value",
				type: "intersection",
				schema: z.object({
					value: z.object({ a: z.string() }).and(z.object({ b: z.number() })),
				}),
			},
			{
				path: "$.value",
				type: "date",
				schema: z.object({ value: z.date() }),
			},
			{
				path: "$.value",
				type: "bigint",
				schema: z.object({ value: z.bigint() }),
			},
			{
				path: "$.value",
				type: "string",
				schema: z.object({
					value: z.string().refine((value) => value.length > 0, "required"),
				}),
			},
			{
				path: "$.value",
				type: "string",
				schema: z.object({ value: z.string().meta({ id: "shared-value" }) }),
			},
		] as const;

		for (const testCase of cases) {
			try {
				zodToJsonSchema(testCase.schema);
				throw new Error(`Expected ${testCase.type} to fail`);
			} catch (error) {
				expect(error).toBeInstanceOf(BindingSchemaConversionError);
				expect(error).toMatchObject({
					path: testCase.path,
					zodType: testCase.type,
				});
			}
		}
	});
});
