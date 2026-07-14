import * as protocol from "@rivet-dev/agentos-runtime-core/protocol";
import {
	decodeBareProtocolFrame,
	encodeBareProtocolFrame,
} from "@rivet-dev/agentos-runtime-core/protocol-frames";
import { SIDECAR_PROTOCOL_SCHEMA } from "@rivet-dev/agentos-runtime-core/protocol-schema";
import { describe, expect, it } from "vitest";
import {
	AcpPendingAbortReason,
	type AcpRequest,
	type AcpResponse,
	AcpRuntimeKind,
	decodeAcpRequest,
	decodeAcpResponse,
	encodeAcpRequest,
	encodeAcpResponse,
} from "../../../core/src/sidecar/agentos-protocol.js";
import {
	type AcpPendingFrameHelpers,
	type AcpPendingResponseHost,
	createAcpPendingResponseDriver,
} from "../../src/acp-pending-driver.js";

const PROCESS_ID = "acp-agent-17";
const REQUEST_ID = 42;
const OWNERSHIP = {
	scope: "vm" as const,
	connection_id: "connection-1",
	session_id: "sidecar-session-1",
	vm_id: "vm-1",
};
const GENERATED_OWNERSHIP = {
	tag: "VmOwnership" as const,
	val: {
		connectionId: "connection-1",
		sessionId: "sidecar-session-1",
		vmId: "vm-1",
	},
};

function responseFrame(requestId: number, response: AcpResponse): Uint8Array {
	const payload = encodeAcpResponse(response).slice().buffer;
	return protocol.encodeProtocolFrame({
		tag: "ResponseFrame",
		val: {
			schema: SIDECAR_PROTOCOL_SCHEMA,
			requestId: BigInt(requestId),
			ownership: GENERATED_OWNERSHIP,
			payload: {
				tag: "ExtEnvelope",
				val: {
					namespace: "dev.rivet.agent-os.acp",
					payload,
				},
			},
		},
	});
}

function requestFrame(request: AcpRequest): Uint8Array {
	return encodeBareProtocolFrame({
		frame_type: "request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: REQUEST_ID,
		ownership: OWNERSHIP,
		payload: {
			type: "ext",
			envelope: {
				namespace: "dev.rivet.agent-os.acp",
				payload: encodeAcpRequest(request),
			},
		},
	});
}

function exerciseProductionDriver(options: {
	request: AcpRequest;
	outputs: string[];
	stderr?: string[];
	finalResponse?: AcpResponse;
	terminal?: "agent_exited" | "interaction_timeout";
	terminalExitCode?: number;
	initialTimeoutMs?: number;
	nextTimeoutMs?: number;
	initialTimeoutPhase?: string;
	nextTimeoutPhase?: string;
	bindError?: Error;
	cancelled?: boolean;
	restart?: {
		processId: string;
		outputs: string[];
		finalResponse: AcpResponse;
	};
}): {
	response: AcpResponse;
	delivered: string[];
	stderrDelivered: string[];
	bindings: string[];
	aborts: Array<{
		processId: string;
		reason: AcpPendingAbortReason;
		exitCode: number | null;
		ownership: typeof GENERATED_OWNERSHIP;
	}>;
	deadlines: number[];
	driverErrors: Array<{ processId: string; message: string }>;
} {
	const outputs = [
		...(options.stderr ?? []).map((output) => ({
			kind: "stderr" as const,
			payload: new TextEncoder().encode(`${output}\n`),
		})),
		...options.outputs.map((output) => ({
			kind: "stdout" as const,
			payload: new TextEncoder().encode(`${output}\n`),
		})),
	];
	const restartOutputs = (options.restart?.outputs ?? []).map((output) => ({
		kind: "stdout" as const,
		payload: new TextEncoder().encode(`${output}\n`),
	}));
	const bindings: string[] = [];
	const deadlines: number[] = [];
	const driverErrors: Array<{ processId: string; message: string }> = [];
	const host: AcpPendingResponseHost = {
		bindPendingProcess(processId) {
			if (options.bindError) throw options.bindError;
			bindings.push(processId);
		},
		pollAgentOutput(processId, deadlineMs) {
			deadlines.push(deadlineMs);
			if (processId === options.restart?.processId) {
				return restartOutputs.shift() ?? null;
			}
			const output = outputs.shift();
			if (output) return output;
			return options.terminal === "agent_exited"
				? {
						kind: "exit",
						payload:
							options.terminalExitCode === undefined
								? new Uint8Array()
								: new TextEncoder().encode(String(options.terminalExitCode)),
					}
				: null;
		},
	};
	const delivered: string[] = [];
	const stderrDelivered: string[] = [];
	const deliveriesByProcess = new Map<string, number>();
	const aborts: Array<{
		processId: string;
		reason: AcpPendingAbortReason;
		exitCode: number | null;
		ownership: typeof GENERATED_OWNERSHIP;
	}> = [];
	let pushCount = 0;
	let internalRequestId = 9_000;
	const frameHelpers: AcpPendingFrameHelpers = {
		pendingResponseProcessId(frame) {
			const decoded = protocol.decodeProtocolFrame(frame);
			if (
				decoded.tag !== "ResponseFrame" ||
				decoded.val.payload.tag !== "ExtEnvelope"
			) {
				return null;
			}
			const response = decodeAcpResponse(
				new Uint8Array(decoded.val.payload.val.payload),
			);
			return response.tag === "AcpPendingResponse"
				? response.val.processId
				: null;
		},
		pendingResponseTimeoutMs(frame) {
			const decoded = protocol.decodeProtocolFrame(frame);
			if (
				decoded.tag !== "ResponseFrame" ||
				decoded.val.payload.tag !== "ExtEnvelope"
			) {
				return null;
			}
			const response = decodeAcpResponse(
				new Uint8Array(decoded.val.payload.val.payload),
			);
			return response.tag === "AcpPendingResponse"
				? response.val.timeoutMs
				: null;
		},
		pendingResponseTimeoutPhase(frame) {
			const decoded = protocol.decodeProtocolFrame(frame);
			if (
				decoded.tag !== "ResponseFrame" ||
				decoded.val.payload.tag !== "ExtEnvelope"
			) {
				return null;
			}
			const response = decodeAcpResponse(
				new Uint8Array(decoded.val.payload.val.payload),
			);
			return response.tag === "AcpPendingResponse"
				? response.val.timeoutPhase
				: null;
		},
		buildDeliverAgentOutputFrame(originResponse, processId, chunk) {
			const origin = protocol.decodeProtocolFrame(originResponse);
			if (origin.tag !== "ResponseFrame") {
				throw new Error("expected origin response");
			}
			internalRequestId += 1;
			return protocol.encodeProtocolFrame({
				tag: "RequestFrame",
				val: {
					schema: origin.val.schema,
					requestId: BigInt(internalRequestId),
					ownership: origin.val.ownership,
					payload: {
						tag: "ExtEnvelope",
						val: {
							namespace: "dev.rivet.agent-os.acp",
							payload: encodeAcpRequest({
								tag: "AcpDeliverAgentOutputRequest",
								val: {
									processId,
									chunk: chunk.slice().buffer,
								},
							}).slice().buffer,
						},
					},
				},
			});
		},
		buildDeliverAgentStderrFrame(originResponse, processId, chunk) {
			const origin = protocol.decodeProtocolFrame(originResponse);
			if (origin.tag !== "ResponseFrame") {
				throw new Error("expected origin response");
			}
			internalRequestId += 1;
			return protocol.encodeProtocolFrame({
				tag: "RequestFrame",
				val: {
					schema: origin.val.schema,
					requestId: BigInt(internalRequestId),
					ownership: origin.val.ownership,
					payload: {
						tag: "ExtEnvelope",
						val: {
							namespace: "dev.rivet.agent-os.acp",
							payload: encodeAcpRequest({
								tag: "AcpDeliverAgentStderrRequest",
								val: { processId, chunk: chunk.slice().buffer },
							}).slice().buffer,
						},
					},
				},
			});
		},
		buildAbortPendingFrame(originResponse, processId, reason, exitCode) {
			const origin = protocol.decodeProtocolFrame(originResponse);
			if (origin.tag !== "ResponseFrame") {
				throw new Error("expected origin response");
			}
			internalRequestId += 1;
			return protocol.encodeProtocolFrame({
				tag: "RequestFrame",
				val: {
					schema: origin.val.schema,
					requestId: BigInt(internalRequestId),
					ownership: origin.val.ownership,
					payload: {
						tag: "ExtEnvelope",
						val: {
							namespace: "dev.rivet.agent-os.acp",
							payload: encodeAcpRequest({
								tag: "AcpAbortPendingRequest",
								val: {
									processId,
									reason:
										reason === "agent_exited"
											? AcpPendingAbortReason.AgentExited
											: reason === "interaction_timeout"
												? AcpPendingAbortReason.InteractionTimeout
												: reason === "caller_cancelled"
													? AcpPendingAbortReason.CallerCancelled
													: AcpPendingAbortReason.DriverFailed,
									exitCode,
								},
							}).slice().buffer,
						},
					},
				},
			});
		},
		restorePendingResponse(originResponse, completedResponse) {
			const origin = protocol.decodeProtocolFrame(originResponse);
			const completed = protocol.decodeProtocolFrame(completedResponse);
			if (origin.tag !== "ResponseFrame" || completed.tag !== "ResponseFrame") {
				throw new Error("expected response frames");
			}
			return protocol.encodeProtocolFrame({
				...completed,
				val: {
					...completed.val,
					schema: origin.val.schema,
					requestId: origin.val.requestId,
					ownership: origin.val.ownership,
				},
			});
		},
	};
	const pushFrame = (frame: Uint8Array): Uint8Array => {
		pushCount += 1;
		if (pushCount === 1) {
			return responseFrame(REQUEST_ID, {
				tag: "AcpPendingResponse",
				val: {
					processId: PROCESS_ID,
					timeoutMs: options.initialTimeoutMs ?? 10_000,
					timeoutPhase: options.initialTimeoutPhase ?? "session/prompt",
				},
			});
		}

		const decoded = protocol.decodeProtocolFrame(frame);
		expect(decoded.tag).toBe("RequestFrame");
		if (decoded.tag !== "RequestFrame")
			throw new Error("expected request frame");
		expect(decoded.val.ownership).toEqual(GENERATED_OWNERSHIP);
		expect(decoded.val.payload.tag).toBe("ExtEnvelope");
		if (decoded.val.payload.tag !== "ExtEnvelope") {
			throw new Error("expected extension request");
		}
		const internal = decodeAcpRequest(
			new Uint8Array(decoded.val.payload.val.payload),
		);
		if (internal.tag === "AcpAbortPendingRequest") {
			aborts.push({
				...internal.val,
				ownership: decoded.val.ownership as typeof GENERATED_OWNERSHIP,
			});
			if (
				options.restart &&
				internal.val.reason === AcpPendingAbortReason.AgentExited
			) {
				return responseFrame(Number(decoded.val.requestId), {
					tag: "AcpPendingResponse",
					val: {
						processId: options.restart.processId,
						timeoutMs: 10_000,
						timeoutPhase: "restart.initialize",
					},
				});
			}
			const code =
				internal.val.reason === AcpPendingAbortReason.AgentExited
					? "agent_exited"
					: internal.val.reason === AcpPendingAbortReason.InteractionTimeout
						? "agent_interaction_timeout"
						: internal.val.reason === AcpPendingAbortReason.CallerCancelled
							? "agent_interaction_cancelled"
							: "agent_driver_failed";
			return responseFrame(Number(decoded.val.requestId), {
				tag: "AcpErrorResponse",
				val: { code, message: `${code} (${PROCESS_ID})` },
			});
		}
		if (internal.tag === "AcpDeliverAgentStderrRequest") {
			stderrDelivered.push(new TextDecoder().decode(internal.val.chunk));
			return responseFrame(Number(decoded.val.requestId), {
				tag: "AcpAgentStderrDeliveredResponse",
				val: { processId: internal.val.processId },
			});
		}
		expect(internal.tag).toBe("AcpDeliverAgentOutputRequest");
		if (internal.tag !== "AcpDeliverAgentOutputRequest") {
			throw new Error("expected ACP internal request");
		}
		expect([PROCESS_ID, options.restart?.processId]).toContain(
			internal.val.processId,
		);
		delivered.push(new TextDecoder().decode(internal.val.chunk));
		const processDeliveryCount =
			(deliveriesByProcess.get(internal.val.processId) ?? 0) + 1;
		deliveriesByProcess.set(internal.val.processId, processDeliveryCount);
		const isRestart = internal.val.processId === options.restart?.processId;
		const expectedOutputs = isRestart
			? (options.restart?.outputs.length ?? 0)
			: options.outputs.length;
		const finalResponse = isRestart
			? options.restart?.finalResponse
			: options.finalResponse;
		const complete = processDeliveryCount === expectedOutputs;
		if (complete && finalResponse === undefined) {
			throw new Error("finalResponse is required for completed delivery");
		}
		if (complete) {
			return responseFrame(Number(decoded.val.requestId), finalResponse);
		}
		return responseFrame(Number(decoded.val.requestId), {
			tag: "AcpPendingResponse",
			val: {
				processId: internal.val.processId,
				timeoutMs: options.nextTimeoutMs ?? 30_000,
				timeoutPhase: options.nextTimeoutPhase ?? "session/prompt",
			},
		});
	};

	const drive = createAcpPendingResponseDriver({
		pushFrame,
		frameHelpers,
		host,
		now: () => 0,
		onDriverError: (processId, message) => {
			driverErrors.push({ processId, message });
		},
		isCancelled: () => options.cancelled ?? false,
	});
	const finalFrame = decodeBareProtocolFrame(
		drive(requestFrame(options.request)),
	);
	expect(finalFrame.frame_type).toBe("response");
	if (finalFrame.frame_type !== "response") {
		throw new Error("expected final response frame");
	}
	expect(finalFrame.request_id).toBe(REQUEST_ID);
	expect(finalFrame.ownership).toEqual(OWNERSHIP);
	expect(finalFrame.payload.type).toBe("ext_result");
	if (finalFrame.payload.type !== "ext_result") {
		throw new Error("expected final extension response");
	}
	return {
		response: decodeAcpResponse(finalFrame.payload.envelope.payload),
		delivered,
		stderrDelivered,
		bindings,
		aborts,
		deadlines,
		driverErrors,
	};
}

describe("production ACP pending-response driver", () => {
	it("turns createSession pending responses into the final created response", () => {
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpCreateSessionRequest",
				val: {
					agentType: "echo",
					runtime: AcpRuntimeKind.JavaScript,
					protocolVersion: 1,
					cwd: "/workspace",
					args: [],
					env: new Map(),
					clientCapabilities: "{}",
					mcpServers: "[]",
					additionalInstructions: null,
					skipOsInstructions: false,
				},
			},
			outputs: ["initialize-result", "session-new-result"],
			initialTimeoutPhase: "create.initialize",
			nextTimeoutPhase: "create.session_new",
			finalResponse: {
				tag: "AcpSessionCreatedResponse",
				val: {
					sessionId: "session-created",
					agentType: "echo",
					processId: PROCESS_ID,
					pid: null,
					modes: null,
					configOptions: [],
					agentCapabilities: null,
					agentInfo: null,
				},
			},
		});
		expect(result.response.tag).toBe("AcpSessionCreatedResponse");
		expect(result.delivered).toEqual([
			"initialize-result\n",
			"session-new-result\n",
		]);
		expect(result.bindings).toEqual([PROCESS_ID]);
		expect(result.deadlines).toEqual([10_000, 30_000]);
	});

	it("forwards adapter stderr to the sidecar-owned event path", () => {
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpSessionRequest",
				val: {
					sessionId: "session-created",
					method: "session/prompt",
					params: JSON.stringify({ prompt: [] }),
				},
			},
			stderr: ["adapter warning"],
			outputs: ["prompt-result"],
			finalResponse: {
				tag: "AcpSessionRpcResponse",
				val: {
					sessionId: "session-created",
					response: "{}",
					text: null,
				},
			},
		});
		expect(result.stderrDelivered).toEqual(["adapter warning\n"]);
		expect(result.delivered).toEqual(["prompt-result\n"]);
	});

	it("turns session prompt pending responses into the final RPC response", () => {
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpSessionRequest",
				val: {
					sessionId: "session-created",
					method: "session/prompt",
					params: JSON.stringify({ prompt: [{ type: "text", text: "hello" }] }),
				},
			},
			outputs: ["prompt-result"],
			initialTimeoutMs: 600_000,
			finalResponse: {
				tag: "AcpSessionRpcResponse",
				val: {
					sessionId: "session-created",
					response: JSON.stringify({ jsonrpc: "2.0", id: 3, result: {} }),
					text: "hello",
				},
			},
		});
		expect(result.response.tag).toBe("AcpSessionRpcResponse");
		expect(result.delivered).toEqual(["prompt-result\n"]);
		expect(result.deadlines).toEqual([600_000]);
	});

	it("drives all resume tiers before returning a resumed response", () => {
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpResumeSessionRequest",
				val: {
					sessionId: "durable-session",
					agentType: "echo",
					transcriptPath: "/history/session.jsonl",
					cwd: "/workspace",
					env: new Map(),
				},
			},
			outputs: [
				"initialize-result",
				"unknown-native-session",
				"fallback-session-new-result",
			],
			finalResponse: {
				tag: "AcpSessionResumedResponse",
				val: {
					sessionId: "fresh-session",
					mode: "fallback",
					agentType: "echo",
					processId: PROCESS_ID,
					pid: null,
				},
			},
		});
		expect(result.response.tag).toBe("AcpSessionResumedResponse");
		expect(result.delivered).toHaveLength(3);
	});

	for (const terminal of ["agent_exited", "interaction_timeout"] as const) {
		it(`atomically aborts a pending interaction after ${terminal}`, () => {
			const result = exerciseProductionDriver({
				request: {
					tag: "AcpSessionRequest",
					val: {
						sessionId: "session-created",
						method: "session/prompt",
						params: JSON.stringify({
							prompt: [{ type: "text", text: "hello" }],
						}),
					},
				},
				outputs: [],
				terminal,
			});
			expect(result.response).toEqual({
				tag: "AcpErrorResponse",
				val: {
					code:
						terminal === "agent_exited"
							? "agent_exited"
							: "agent_interaction_timeout",
					message: expect.stringContaining(PROCESS_ID),
				},
			});
			expect(result.aborts).toEqual([
				{
					processId: PROCESS_ID,
					reason:
						terminal === "agent_exited"
							? AcpPendingAbortReason.AgentExited
							: AcpPendingAbortReason.InteractionTimeout,
					ownership: GENERATED_OWNERSHIP,
					exitCode: null,
				},
			]);
		});
	}

	it("forwards host cancellation to sidecar-owned atomic cleanup", () => {
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpSessionRequest",
				val: {
					sessionId: "session-created",
					method: "session/prompt",
					params: JSON.stringify({ prompt: [] }),
				},
			},
			outputs: [],
			cancelled: true,
		});
		expect(result.response).toEqual({
			tag: "AcpErrorResponse",
			val: {
				code: "agent_interaction_cancelled",
				message: expect.stringContaining(PROCESS_ID),
			},
		});
		expect(result.aborts).toEqual([
			{
				processId: PROCESS_ID,
				reason: AcpPendingAbortReason.CallerCancelled,
				ownership: GENERATED_OWNERSHIP,
				exitCode: null,
			},
		]);
	});

	it("drives a sidecar-issued restart continuation without owning restart policy", () => {
		const replacementProcessId = "acp-agent-18";
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpSessionRequest",
				val: {
					sessionId: "session-created",
					method: "session/prompt",
					params: JSON.stringify({ prompt: [] }),
				},
			},
			outputs: [],
			terminal: "agent_exited",
			terminalExitCode: 137,
			restart: {
				processId: replacementProcessId,
				outputs: ["replacement-initialize", "replacement-load"],
				finalResponse: {
					tag: "AcpErrorResponse",
					val: {
						code: "invalid_state",
						message: "adapter restarted; retry the request",
					},
				},
			},
		});

		expect(result.response).toEqual({
			tag: "AcpErrorResponse",
			val: {
				code: "invalid_state",
				message: "adapter restarted; retry the request",
			},
		});
		expect(result.bindings).toEqual([PROCESS_ID, replacementProcessId]);
		expect(result.delivered).toEqual([
			"replacement-initialize\n",
			"replacement-load\n",
		]);
		expect(result.aborts).toEqual([
			{
				processId: PROCESS_ID,
				reason: AcpPendingAbortReason.AgentExited,
				ownership: GENERATED_OWNERSHIP,
				exitCode: 137,
			},
		]);
	});

	it("reports setup failures through the sidecar-owned driver-failed error", () => {
		const result = exerciseProductionDriver({
			request: {
				tag: "AcpSessionRequest",
				val: {
					sessionId: "session-created",
					method: "session/prompt",
					params: JSON.stringify({
						prompt: [{ type: "text", text: "hello" }],
					}),
				},
			},
			outputs: [],
			bindError: new Error("host binding failed"),
		});
		expect(result.response).toEqual({
			tag: "AcpErrorResponse",
			val: {
				code: "agent_driver_failed",
				message: expect.stringContaining(PROCESS_ID),
			},
		});
		expect(result.aborts).toEqual([
			{
				processId: PROCESS_ID,
				reason: AcpPendingAbortReason.DriverFailed,
				ownership: GENERATED_OWNERSHIP,
				exitCode: null,
			},
		]);
		expect(result.driverErrors).toEqual([
			{ processId: PROCESS_ID, message: "host binding failed" },
		]);
	});
});
