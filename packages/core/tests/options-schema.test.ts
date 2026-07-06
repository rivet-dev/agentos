import { describe, expect, test } from "vitest";
import { AgentOs, agentOsOptionsSchema } from "../src/index.js";
import { parseAgentOsOptions } from "../src/options-schema.js";
import {
	getSandboxDisposeHooks,
	resolveSandboxOptions,
} from "../src/sandbox.js";

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

	test("provider sandbox starts a client and owns disposal", async () => {
		let disposed = false;
		const client = {
			baseUrl: "http://127.0.0.1:1234",
			dispose: () => {
				disposed = true;
			},
		} as never;

		const options = await resolveSandboxOptions(
			parseAgentOsOptions({
				sandbox: {
					provider: {
						start: async () => client,
					},
				},
			}),
		);
		expect(options).not.toHaveProperty("sandbox");
		expect(options.mounts?.[0]?.path).toBe("/mnt/sandbox");
		expect(options.toolKits?.[0]?.name).toBe("sandbox");

		for (const hook of getSandboxDisposeHooks(options)) {
			await hook();
		}
		expect(disposed).toBe(true);
	});

	test("advanced sandbox client leaves disposal manual by default", async () => {
		const client = { baseUrl: "http://127.0.0.1:1234" } as never;
		const parsed = parseAgentOsOptions({
			sandbox: {
				client,
				mountPath: "/work",
			},
		});

		const options = await resolveSandboxOptions(parsed);
		expect(options.mounts?.[0]?.path).toBe("/work");
		expect(getSandboxDisposeHooks(options)).toHaveLength(0);
	});

	test("rejects removed sandbox mount and binding toggles", async () => {
		const client = { baseUrl: "http://127.0.0.1:1234" } as never;
		await expect(
			resolveSandboxOptions(
				parseAgentOsOptions({
					sandbox: {
						client,
						mount: false,
					} as never,
				}),
			),
		).rejects.toThrow(/sandbox\.mount has been removed/);

		await expect(
			resolveSandboxOptions(
				parseAgentOsOptions({
					sandbox: {
						client,
						bindings: false,
					} as never,
				}),
			),
		).rejects.toThrow(/sandbox\.bindings has been removed/);
	});

	test("rejects old sandbox path option names", async () => {
		const client = { baseUrl: "http://127.0.0.1:1234" } as never;
		await expect(
			resolveSandboxOptions(
				parseAgentOsOptions({
					sandbox: {
						client,
						basePath: "/app",
					} as never,
				}),
			),
		).rejects.toThrow(/sandbox\.basePath has been removed/);
	});
});
