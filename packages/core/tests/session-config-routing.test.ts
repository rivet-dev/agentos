import { describe, expect, it, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import type {
	AcpRequest,
	AcpResponse,
} from "../src/sidecar/agentos-protocol.js";

describe("AgentOs session config routing", () => {
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
