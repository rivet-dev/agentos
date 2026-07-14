import { describe, expect, test } from "vitest";
import { z } from "zod";
import {
	HostToolSchemaConversionError,
	zodToJsonSchema,
} from "../src/host-tools-zod.js";
import { hostTool } from "../src/index.js";

describe("host tool Zod conversion", () => {
	test("fails loudly when a host tool input schema uses an unsupported discriminated union", () => {
		const tool = hostTool({
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
			zodToJsonSchema(tool.inputSchema);
			throw new Error("Expected unsupported host tool schema to fail");
		} catch (error) {
			expect(error).toBeInstanceOf(HostToolSchemaConversionError);
			expect(error).toMatchObject({
				path: "$.payload",
				zodType: "discriminatedUnion",
			});
		}
	});
});
