import { AgentOs } from "@rivet-dev/agentos";
import { afterEach, expect, test, vi } from "vitest";
import { createVm, execute } from "../src/index.js";

afterEach(() => vi.restoreAllMocks());

test("one-shot calls select secure defaults and forward VM options", async () => {
	const result = { outcome: "succeeded", exitCode: 0 };
	const executeGuest = vi.fn().mockResolvedValue(result);
	const dispose = vi.fn().mockResolvedValue(undefined);
	const create = vi.spyOn(AgentOs, "create").mockResolvedValue({
		javascript: { execute: executeGuest, npm: {} },
		typescript: {},
		filesystem: {},
		network: {},
		process: {},
		dispose,
	} as unknown as AgentOs);

	await expect(
		execute("console.log('test')", {
			defaultSoftware: false,
			environment: { CUSTOM: "value" },
			wasmBackend: "wasmtime",
			timeoutMs: 100,
		}),
	).resolves.toBe(result);

	expect(create).toHaveBeenCalledWith({
		defaultSoftware: false,
		defaultsProfile: "secure",
		environment: { CUSTOM: "value" },
		wasmBackend: "wasmtime",
	});
	expect(executeGuest).toHaveBeenCalledWith("console.log('test')", {
		timeoutMs: 100,
	});
	expect(dispose).toHaveBeenCalledOnce();
});

test("undefined selects secure defaults and an explicit profile is retained", async () => {
	const dispose = vi.fn().mockResolvedValue(undefined);
	const create = vi.spyOn(AgentOs, "create").mockResolvedValue({
		javascript: {
			npm: {},
			execute: vi.fn().mockResolvedValue({ outcome: "succeeded" }),
		},
		dispose,
	} as unknown as AgentOs);
	const vm = await createVm({ defaultsProfile: undefined });
	await vm.dispose();
	await execute("", { defaultsProfile: undefined });
	await execute("", { defaultsProfile: "agent_os" });
	expect(
		create.mock.calls.map(([options]) => options?.defaultsProfile),
	).toEqual(["secure", "secure", "agent_os"]);
});
