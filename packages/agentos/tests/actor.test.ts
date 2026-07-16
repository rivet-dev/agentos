import { describe, expect, test, vi } from "vitest";
import { event } from "rivetkit";
import { agentOS } from "../src/index.js";

describe("agentOS actor", () => {
	test("is a normal actor with built-in and user-defined actions", () => {
		const definition = agentOS({
			createState: () => ({ count: 0 }),
			events: { countChanged: event<{ count: number }>() },
			actions: {
				increment: (c, amount: number) => {
					c.state.count += amount;
					return c.state.count;
				},
			},
		});

		expect(definition.config.actions).toHaveProperty("increment");
		expect(definition.config.actions).toHaveProperty("readFile");
		expect(definition.config.actions).toHaveProperty("createSession");
		expect(definition.config.events).toHaveProperty("countChanged");
		expect(definition.config.events).toHaveProperty("vmBooted");
		expect(definition.config.events).toHaveProperty("sessionEvent");
	});

	test("preserves normal actor connection hooks", async () => {
		const onBeforeConnect = vi.fn();
		const definition = agentOS({ onBeforeConnect });
		await definition.config.onBeforeConnect?.(
			{ request: undefined } as never,
			undefined,
		);
		expect(onBeforeConnect).toHaveBeenCalledOnce();
	});

	test("rejects collisions with AgentOS defaults", () => {
		expect(() =>
			agentOS({
				actions: { readFile: () => "shadowed" },
			} as never),
		).toThrow("agentOS() action name is reserved: readFile");
	});

	test("keeps AgentOS limits bounded by default", () => {
		const definition = agentOS();
		expect(definition.config.options.actionTimeout).toBe(15 * 60_000);
		expect(definition.config.options.sleepGracePeriod).toBe(15 * 60_000);
	});
});
