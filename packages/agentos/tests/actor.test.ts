import { AgentOs } from "@rivet-dev/agentos-core";
import {
	AGENT_OS_CONFORMANCE_ACTIONS,
	AGENT_OS_CONFORMANCE_EVENTS,
} from "@rivet-dev/agentos-test-harness/agent-os-conformance";
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

	test("keeps the shared conformance inventory in lockstep with actor built-ins", () => {
		const actions = createAgentOsActions();
		expect(Object.keys(actions).sort()).toEqual(
			[
				...AGENT_OS_CONFORMANCE_ACTIONS,
				"createSignedPreviewUrl",
				"expireSignedPreviewUrl",
			].sort(),
		);
		const definition = agentOS();
		expect(Object.keys(definition.config.events ?? {}).sort()).toEqual(
			[...AGENT_OS_CONFORMANCE_EVENTS, "vmBooted", "vmShutdown"].sort(),
		);
	});

	test("creates and expires actor-only signed preview URLs", async () => {
		const execute = vi.fn(async () => []);
		const actions = createAgentOsActions();
		const context = { db: { execute } } as never;
		const preview = await actions.createSignedPreviewUrl(context, 8080, 60);
		expect(preview).toMatchObject({
			path: `/fetch/${preview.token}`,
			port: 8080,
		});
		expect(preview.expiresAt).toBeGreaterThan(Date.now());
		expect(execute).toHaveBeenCalledWith(
			expect.stringContaining("INSERT INTO agent_os_preview_tokens"),
			preview.token,
			8080,
			expect.any(Number),
			preview.expiresAt,
		);

		await actions.expireSignedPreviewUrl(context, preview.token);
		expect(execute).toHaveBeenLastCalledWith(
			expect.stringContaining("DELETE FROM agent_os_preview_tokens"),
			preview.token,
		);
	});

	test("bounds active preview tokens and serializes mount descriptors", async () => {
		const execute = vi.fn(async (query: string) =>
			query.includes("COUNT(*)") ? [{ count: 1 }] : [],
		);
		const actions = createAgentOsActions(
			{
				mounts: [
					{
						path: "/fixture",
						plugin: {
							id: "host_dir",
							config: {
								hostPath: "/tmp/fixture",
								readOnly: true,
								secretAccessKey: "must-not-leak",
							},
						},
						readOnly: true,
					},
					{
						path: "/readonly-overlay",
						filesystem: {
							type: "overlay",
							store: {} as never,
							mode: "read-only",
							lowers: [],
						},
					},
				],
			},
			{},
			{ maxActiveTokens: 1 },
		);
		const context = { db: { execute } } as never;
		await expect(
			actions.createSignedPreviewUrl(context, 8080, 60),
		).rejects.toThrow(
			"preview token limit 1 reached; raise preview.maxActiveTokens",
		);
		expect(await actions.listMounts(context)).toEqual([
			{
				path: "/fixture",
				kind: "host_dir",
				readOnly: true,
			},
			{
				path: "/readonly-overlay",
				kind: "overlay",
				readOnly: true,
				config: { mode: "read-only" },
			},
		]);
	});

	test("returns public typed preview errors and warns near the token limit", async () => {
		try {
			createAgentOsActions({}, {}, { maxActiveTokens: 0 });
			throw new Error("expected invalid preview config to fail");
		} catch (error) {
			expect(error).toMatchObject({
				code: "agentos_preview_invalid_config",
				public: true,
			});
		}

		const execute = vi.fn(async (query: string) =>
			query.includes("COUNT(*)") ? [{ count: 3 }] : [],
		);
		const warn = vi.fn();
		const actions = createAgentOsActions(
			{},
			{},
			{
				defaultExpiresInSeconds: 10,
				maxExpiresInSeconds: 60,
				maxActiveTokens: 5,
			},
		);
		const context = { db: { execute }, log: { warn } } as never;

		await expect(
			actions.createSignedPreviewUrl(context, 0, 10),
		).rejects.toMatchObject({
			code: "agentos_preview_invalid_port",
			public: true,
		});
		await expect(
			actions.createSignedPreviewUrl(context, 8080, 61),
		).rejects.toMatchObject({
			code: "agentos_preview_invalid_ttl",
			public: true,
		});
		await expect(
			actions.createSignedPreviewUrl(context, 8080, 10),
		).resolves.toMatchObject({ port: 8080 });
		expect(warn).toHaveBeenCalledWith(
			expect.objectContaining({
				activeTokenCount: 4,
				limit: 5,
				msg: expect.stringContaining("raise preview.maxActiveTokens"),
			}),
		);
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

	test("only bypasses onBeforeConnect for well-formed preview URLs", async () => {
		const onBeforeConnect = vi.fn();
		const definition = agentOS({ onBeforeConnect });
		const token = "a".repeat(48);
		await definition.config.onBeforeConnect?.(
			{
				request: new Request(`https://actor.test/fetch/${token}/path`),
			} as never,
			undefined,
		);
		expect(onBeforeConnect).not.toHaveBeenCalled();

		await definition.config.onBeforeConnect?.(
			{ request: new Request("https://actor.test/fetch/not-a-token") } as never,
			undefined,
		);
		expect(onBeforeConnect).toHaveBeenCalledOnce();
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

	test("logs adapter crashes and releases terminal session holds", async () => {
		let onAgentExit:
			| ((event: {
					sessionId: string;
					agentType: string;
					processId: string;
					pid: number | null;
					exitCode: number | null;
					restart: "unsupported" | "failed" | "exhausted";
					restartCount: number;
					maxRestarts: number;
			  }) => void)
			| undefined;
		let sessionIndex = 0;
		const vm = {
			onCronEvent: vi.fn(),
			createSession: vi.fn(async () => ({
				sessionId: `terminal-session-${++sessionIndex}`,
			})),
			onSessionEvent: vi.fn(),
			onPermissionRequest: vi.fn(),
		};
		vi.spyOn(AgentOs, "create").mockImplementation(async (options) => {
			onAgentExit = options.onAgentExit as typeof onAgentExit;
			return vm as never;
		});
		const released: string[] = [];
		const log = { info: vi.fn(), error: vi.fn() };
		const context = {
			actorId: "terminal-exit-test",
			actorUds: vi.fn(async () => ({
				path: "/tmp/actor.sock",
				token: "token",
			})),
			broadcast: vi.fn(),
			db: { execute: vi.fn(async () => []) },
			keepAwake: vi.fn(async (hold: Promise<void>) => {
				await hold;
				released.push("released");
			}),
			waitUntil: vi.fn(),
			log,
		} as never;
		const userOnAgentExit = vi.fn(() => {
			throw new Error("hook failure");
		});
		const actions = createAgentOsActions({ onAgentExit: userOnAgentExit });
		for (const restart of ["unsupported", "failed", "exhausted"] as const) {
			const sessionId = await actions.createSession(context, "test-agent");
			onAgentExit?.({
				sessionId,
				agentType: "test-agent",
				processId: `process-${restart}`,
				pid: 1,
				exitCode: 1,
				restart,
				restartCount: 1,
				maxRestarts: 1,
			});
		}
		await vi.waitFor(() => expect(released).toHaveLength(3));
		expect(userOnAgentExit).toHaveBeenCalledTimes(3);
		expect(log.error).toHaveBeenCalledWith(
			expect.objectContaining({
				msg: "agent-os agent adapter exited unexpectedly",
			}),
		);
		expect(log.error).toHaveBeenCalledWith(
			expect.objectContaining({ msg: "agent-os onAgentExit hook failed" }),
		);
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
