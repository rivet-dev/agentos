import { describe, expect, it, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import type {
	AcpRequest,
	AcpResponse,
} from "../src/sidecar/agentos-protocol.js";

describe("AgentOs session config routing", () => {
	it("uses only the sidecar-owned post-decision cleanup deadline", async () => {
		vi.useFakeTimers();
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
		try {
			const agent = Object.create(AgentOs.prototype) as AgentOs;
			const permissionHandlers = new Set<() => void>();
			permissionHandlers.add(() => {});
			(
				agent as unknown as {
					_sessions: Map<string, unknown>;
				}
			)._sessions = new Map([
				[
					"session-1",
					{
						sessionId: "session-1",
						permissionHandlers,
						pendingPermissionReplies: new Map(),
					},
				],
			]);

			const callback = (
				agent as unknown as {
					_handleAcpPermissionCallback: (
						sessionId: string,
						permissionId: string,
						params: Record<string, unknown>,
						cleanupAfterMs: number,
					) => Promise<unknown>;
				}
			)._handleAcpPermissionCallback("session-1", "permission-1", {}, 20);
			let settled = false;
			void callback.then(() => {
				settled = true;
			});

			await vi.advanceTimersByTimeAsync(10);
			expect(settled).toBe(false);

			await vi.advanceTimersByTimeAsync(10);
			expect(settled).toBe(true);
			await expect(callback).resolves.toBeUndefined();
			await expect(
				agent.respondPermission("session-1", "permission-1", "once"),
			).rejects.toThrow("Permission request is not pending: permission-1");
		} finally {
			warn.mockRestore();
			vi.useRealTimers();
		}
	});

	it("forwards category and value without interpreting adapter config metadata", async () => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const sendAcpRequest = vi.fn(
			async (_request: AcpRequest): Promise<AcpResponse> => ({
				tag: "AcpSessionRpcResponse",
				val: {
					sessionId: "session-1",
					text: null,
					response: JSON.stringify({
						jsonrpc: "2.0",
						id: null,
						result: null,
					}),
				},
			}),
		);
		(
			agent as unknown as {
				_sendAcpRequest: typeof sendAcpRequest;
			}
		)._sendAcpRequest = sendAcpRequest;

		await agent.setSessionModel("session-1", "model-1");

		expect(sendAcpRequest).toHaveBeenCalledWith({
			tag: "AcpSetSessionConfigRequest",
			val: {
				sessionId: "session-1",
				category: "model",
				value: "model-1",
			},
		});
	});

	it("forwards session requests without a client lifecycle gate or cancel fallback", async () => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const sendAcpRequest = vi.fn(
			async (_request: AcpRequest): Promise<AcpResponse> => ({
				tag: "AcpSessionRpcResponse",
				val: {
					sessionId: "sidecar-only-session",
					text: null,
					response: JSON.stringify({
						jsonrpc: "2.0",
						id: 1,
						error: { code: -32601, message: "adapter response" },
					}),
				},
			}),
		);
		(
			agent as unknown as {
				_sendAcpRequest: typeof sendAcpRequest;
			}
		)._sendAcpRequest = sendAcpRequest;

		const response = await agent.cancelSession("sidecar-only-session");

		expect(response.error).toMatchObject({
			code: -32601,
			message: "adapter response",
		});
		expect(response.result).toBeUndefined();
		expect(sendAcpRequest).toHaveBeenCalledWith({
			tag: "AcpSessionRequest",
			val: {
				sessionId: "sidecar-only-session",
				method: "session/cancel",
				params: null,
			},
		});
	});

	it("destroy uses the sidecar close path without client cancel orchestration", async () => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const sendAcpRequest = vi.fn(
			async (request: AcpRequest): Promise<AcpResponse> => {
				if (request.tag !== "AcpCloseSessionRequest") {
					throw new Error(`unexpected request ${request.tag}`);
				}
				return {
					tag: "AcpSessionClosedResponse",
					val: { sessionId: request.val.sessionId },
				};
			},
		);
		const backdoor = agent as unknown as {
			_sessions: Map<string, unknown>;
			_sendAcpRequest: typeof sendAcpRequest;
		};
		backdoor._sessions = new Map();
		backdoor._sendAcpRequest = sendAcpRequest;

		await agent.destroySession("sidecar-only-session");

		expect(sendAcpRequest).toHaveBeenCalledOnce();
		expect(sendAcpRequest).toHaveBeenCalledWith({
			tag: "AcpCloseSessionRequest",
			val: { sessionId: "sidecar-only-session" },
		});
	});

	it.each([
		{
			name: "a transport failure",
			fail: async (): Promise<AcpResponse> => {
				throw new Error("transport closed");
			},
			error: "transport closed",
		},
		{
			name: "an unexpected response",
			fail: async (): Promise<AcpResponse> => ({
				tag: "AcpListSessionsResponse",
				val: { sessions: [] },
			}),
			error: "unexpected response to AcpCloseSessionRequest",
		},
		{
			name: "a mismatched session id",
			fail: async (): Promise<AcpResponse> => ({
				tag: "AcpSessionClosedResponse",
				val: { sessionId: "other-session" },
			}),
			error: "unexpected session id in AcpSessionClosedResponse",
		},
	])("retains ACP routes after $name and removes them after a confirmed retry", async ({
		fail,
		error,
	}) => {
		const sessionId = "session-1";
		const rejectPendingPermission = vi.fn();
		const cleanupTimer = setTimeout(() => {}, 60_000);
		const session = {
			pendingPermissionReplies: new Map([
				[
					"permission-1",
					{
						resolve: vi.fn(),
						reject: rejectPendingPermission,
						cleanupTimer,
					},
				],
			]),
		};
		let attempt = 0;
		const sendAcpRequest = vi.fn(
			async (request: AcpRequest): Promise<AcpResponse> => {
				attempt += 1;
				if (attempt === 1) {
					return fail();
				}
				if (request.tag !== "AcpCloseSessionRequest") {
					throw new Error(`unexpected request: ${request.tag}`);
				}
				return {
					tag: "AcpSessionClosedResponse",
					val: { sessionId: request.val.sessionId },
				};
			},
		);
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const backdoor = agent as unknown as {
			_sessions: Map<string, typeof session>;
			_sendAcpRequest: typeof sendAcpRequest;
		};
		backdoor._sessions = new Map([[sessionId, session]]);
		backdoor._sendAcpRequest = sendAcpRequest;

		try {
			await expect(agent.closeSession(sessionId)).rejects.toThrow(error);
			expect(backdoor._sessions.get(sessionId)).toBe(session);
			expect(session.pendingPermissionReplies.has("permission-1")).toBe(true);
			expect(rejectPendingPermission).not.toHaveBeenCalled();

			await expect(agent.closeSession(sessionId)).resolves.toBeUndefined();
			expect(backdoor._sessions.has(sessionId)).toBe(false);
			expect(session.pendingPermissionReplies.size).toBe(0);
			expect(rejectPendingPermission).toHaveBeenCalledOnce();
			expect(rejectPendingPermission).toHaveBeenCalledWith(
				expect.objectContaining({
					message: "Session closed before permission reply: permission-1",
				}),
			);
		} finally {
			clearTimeout(cleanupTimer);
		}
	});

	it("uses sidecar-accumulated prompt text without a client event route", async () => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const sendAcpRequest = vi.fn(
			async (_request: AcpRequest): Promise<AcpResponse> => ({
				tag: "AcpSessionRpcResponse",
				val: {
					sessionId: "sidecar-only-session",
					response: JSON.stringify({
						jsonrpc: "2.0",
						id: 1,
						result: { stopReason: "end_turn" },
					}),
					text: "sidecar text",
				},
			}),
		);
		(
			agent as unknown as {
				_sendAcpRequest: typeof sendAcpRequest;
			}
		)._sendAcpRequest = sendAcpRequest;

		await expect(
			agent.prompt("sidecar-only-session", "hello"),
		).resolves.toEqual({
			response: expect.objectContaining({ jsonrpc: "2.0" }),
			text: "sidecar text",
		});
	});
});
