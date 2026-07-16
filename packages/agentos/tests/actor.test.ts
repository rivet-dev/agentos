import { AgentOs } from "@rivet-dev/agentos-core";
import { event } from "rivetkit";
import { describe, expect, test, vi } from "vitest";
import { agentOS, createAgentOsActions } from "../src/index.js";

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
		expect(definition.config.actions).toHaveProperty("cancelPrompt");
		expect(definition.config.actions).toHaveProperty("destroySession");
		expect(definition.config.actions).toHaveProperty("setModel");
		expect(definition.config.actions).toHaveProperty("listSessions");
		expect(definition.config.events).toHaveProperty("countChanged");
		expect(definition.config.events).toHaveProperty("vmBooted");
		expect(definition.config.events).toHaveProperty("sessionEvent");
	});

	test("preserves normal actor connection hooks", async () => {
		const onBeforeConnect = vi.fn();
		const onConnect = vi.fn();
		const onDisconnect = vi.fn();
		const createConnState = vi.fn(() => ({ authenticated: true }));
		const definition = agentOS({
			onBeforeConnect,
			onConnect,
			onDisconnect,
			createConnState,
		});
		await definition.config.onBeforeConnect?.(
			{ request: undefined } as never,
			undefined,
		);
		expect(onBeforeConnect).toHaveBeenCalledOnce();
		expect(definition.config.onConnect).toBe(onConnect);
		expect(definition.config.onDisconnect).toBe(onDisconnect);
		expect(definition.config.createConnState).toBe(createConnState);
	});

	test("runs native session and permission hooks with actor context", async () => {
		let emitSessionEvent: ((event: unknown) => void) | undefined;
		let emitPermissionRequest: ((request: unknown) => void) | undefined;
		const vm = {
			onCronEvent: vi.fn(),
			createSession: vi.fn(async () => ({ sessionId: "session-1" })),
			onSessionEvent: vi.fn((_sessionId, callback) => {
				emitSessionEvent = callback;
			}),
			onPermissionRequest: vi.fn((_sessionId, callback) => {
				emitPermissionRequest = callback;
			}),
		};
		vi.spyOn(AgentOs, "create").mockResolvedValue(vm as never);

		const onSessionEvent = vi.fn();
		const onPermissionRequest = vi.fn();
		const actions = createAgentOsActions(
			{},
			{ onSessionEvent, onPermissionRequest },
		);
		const pending: Promise<unknown>[] = [];
		const context = {
			actorId: "hook-test",
			actorUds: vi.fn(async () => ({
				path: "/tmp/actor.sock",
				token: "token",
			})),
			broadcast: vi.fn(),
			db: { execute: vi.fn(async () => []) },
			keepAwake: <T>(promise: Promise<T>) => promise,
			waitUntil: (promise: Promise<unknown>) => pending.push(promise),
			log: { info: vi.fn(), error: vi.fn() },
		} as never;

		await actions.createSession(context, "test-agent");
		emitSessionEvent?.({ jsonrpc: "2.0", method: "session/update" });
		emitPermissionRequest?.({ permissionId: "permission-1", params: {} });
		await Promise.all(pending);

		expect(onSessionEvent).toHaveBeenCalledWith(context, "session-1", {
			jsonrpc: "2.0",
			method: "session/update",
		});
		expect(onPermissionRequest).toHaveBeenCalledWith(context, "session-1", {
			permissionId: "permission-1",
			params: {},
		});
		expect(context.db.execute).not.toHaveBeenCalled();
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
