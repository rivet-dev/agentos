import { describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	type AcpResponse,
	encodeAcpEvent,
	encodeAcpResponse,
} from "../src/sidecar/agentos-protocol.js";

const ACP_EXTENSION_NAMESPACE = "dev.rivet.agent-os.acp";

type RouteResponse = Extract<
	AcpResponse,
	{
		tag: "AcpSessionCreatedResponse" | "AcpSessionResumedResponse";
	}
>;

function createInjectedAgent(response: RouteResponse) {
	const agent = Object.create(AgentOs.prototype) as AgentOs;
	const onSessionEvent = vi.fn();
	let requestCount = 0;
	const responseEnvelope = {
		namespace: ACP_EXTENSION_NAMESPACE,
		payload: encodeAcpResponse(response),
	};
	const backdoor = agent as unknown as {
		_sessions: Map<
			string,
			{
				pid: number | null;
				eventHandlers: Set<{
					handler: (notification: unknown) => void;
				}>;
			}
		>;
		_sidecarSession: unknown;
		_sidecarVm: unknown;
		_handleAcpExtEvent(envelope: {
			namespace: string;
			payload: Uint8Array;
		}): void;
		_sidecarClient: {
			extensionRequest(
				session: unknown,
				vm: unknown,
				request: { namespace: string; payload: Uint8Array },
				options?: {
					onResponse?: (envelope: {
						namespace: string;
						payload: Uint8Array;
					}) => void;
				},
			): Promise<{ namespace: string; payload: Uint8Array }>;
		};
	};
	const sessions: typeof backdoor._sessions = new Map();
	const setSession = sessions.set.bind(sessions);
	sessions.set = (sessionId, route) => {
		route.eventHandlers.add({ handler: onSessionEvent });
		return setSession(sessionId, route);
	};
	backdoor._sessions = sessions;
	backdoor._sidecarSession = {};
	backdoor._sidecarVm = {};
	backdoor._sidecarClient = {
		async extensionRequest(_session, _vm, _request, options) {
			requestCount += 1;
			options?.onResponse?.(responseEnvelope);
			const route = backdoor._sessions.get(response.val.sessionId);
			expect(route?.pid).toBe(response.val.pid);

			backdoor._handleAcpExtEvent({
				namespace: ACP_EXTENSION_NAMESPACE,
				payload: encodeAcpEvent({
					tag: "AcpSessionEvent",
					val: {
						sessionId: response.val.sessionId,
						notification: JSON.stringify({
							jsonrpc: "2.0",
							method: "session/update",
							params: { phase: "ready" },
						}),
					},
				}),
			});
			return responseEnvelope;
		},
	};

	return {
		agent,
		onSessionEvent,
		requestCount: () => requestCount,
	};
}

describe("ACP session route registration", () => {
	test("create binds the response route before a following event without a state RPC", async () => {
		const injected = createInjectedAgent({
			tag: "AcpSessionCreatedResponse",
			val: {
				sessionId: "created-session",
				agentType: "test-agent",
				processId: "process-created",
				pid: 42,
				modes: null,
				configOptions: [],
				agentCapabilities: null,
				agentInfo: null,
			},
		});

		await expect(injected.agent.createSession("test-agent")).resolves.toEqual({
			sessionId: "created-session",
		});
		expect(injected.requestCount()).toBe(1);
		expect(injected.onSessionEvent).toHaveBeenCalledWith(
			expect.objectContaining({
				method: "session/update",
				params: { phase: "ready" },
			}),
		);
	});

	test("resume binds the live response route before a following event without a state RPC", async () => {
		const injected = createInjectedAgent({
			tag: "AcpSessionResumedResponse",
			val: {
				sessionId: "live-session",
				mode: "fallback",
				agentType: "test-agent",
				processId: "process-resumed",
				pid: 43,
			},
		});

		await expect(
			injected.agent.resumeSession("external-session", "test-agent"),
		).resolves.toEqual({
			sessionId: "live-session",
			mode: "fallback",
		});
		expect(injected.requestCount()).toBe(1);
		expect(injected.onSessionEvent).toHaveBeenCalledWith(
			expect.objectContaining({
				method: "session/update",
				params: { phase: "ready" },
			}),
		);
	});

	test("a response-hook validation failure leaves no local route", async () => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const sessions = new Map<string, unknown>();
		const responseEnvelope = {
			namespace: ACP_EXTENSION_NAMESPACE,
			payload: encodeAcpResponse({
				tag: "AcpListSessionsResponse",
				val: { sessions: [] },
			}),
		};
		const backdoor = agent as unknown as {
			_sessions: typeof sessions;
			_sidecarSession: unknown;
			_sidecarVm: unknown;
			_sidecarClient: {
				extensionRequest(
					session: unknown,
					vm: unknown,
					request: unknown,
					options?: {
						onResponse?: (envelope: typeof responseEnvelope) => void;
					},
				): Promise<typeof responseEnvelope>;
			};
		};
		backdoor._sessions = sessions;
		backdoor._sidecarSession = {};
		backdoor._sidecarVm = {};
		backdoor._sidecarClient = {
			async extensionRequest(_session, _vm, _request, options) {
				options?.onResponse?.(responseEnvelope);
				return responseEnvelope;
			},
		};

		await expect(agent.createSession("test-agent")).rejects.toThrow(
			"unexpected create_session response: AcpListSessionsResponse",
		);
		expect(sessions.size).toBe(0);
	});

	test("registers the route after injected transports omit the optional response hook", async () => {
		const agent = Object.create(AgentOs.prototype) as AgentOs;
		const sessions = new Map<string, unknown>();
		const responseEnvelope = {
			namespace: ACP_EXTENSION_NAMESPACE,
			payload: encodeAcpResponse({
				tag: "AcpSessionCreatedResponse",
				val: {
					sessionId: "injected-session",
					agentType: "test-agent",
					processId: "injected-process",
					pid: 44,
					modes: null,
					configOptions: [],
					agentCapabilities: null,
					agentInfo: null,
				},
			}),
		};
		const backdoor = agent as unknown as {
			_sessions: typeof sessions;
			_sidecarSession: unknown;
			_sidecarVm: unknown;
			_sidecarClient: {
				extensionRequest(): Promise<typeof responseEnvelope>;
			};
		};
		backdoor._sessions = sessions;
		backdoor._sidecarSession = {};
		backdoor._sidecarVm = {};
		backdoor._sidecarClient = {
			async extensionRequest() {
				return responseEnvelope;
			},
		};

		await expect(agent.createSession("test-agent")).resolves.toEqual({
			sessionId: "injected-session",
		});
		expect(sessions.has("injected-session")).toBe(true);
	});
});
