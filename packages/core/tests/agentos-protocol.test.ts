import { describe, expect, test } from "vitest";
import {
	type AcpRequest,
	type AcpResponse,
	AcpRuntimeKind,
	decodeAcpRequest,
	decodeAcpResponse,
	encodeAcpRequest,
	encodeAcpResponse,
} from "../src/sidecar/agentos-protocol.js";

describe("agent-os ACP protocol", () => {
	test("round-trips create-session requests", () => {
		const request: AcpRequest = {
			tag: "AcpCreateSessionRequest",
			val: {
				agentType: "codex",
				runtime: AcpRuntimeKind.JavaScript,
				cwd: "/home/agentos",
				args: ["--model", "gpt-5"],
				env: new Map([["AGENTOS_KEEP_STDIN_OPEN", "1"]]),
				protocolVersion: 1,
				clientCapabilities: "{}",
				mcpServers: "{}",
				skipOsInstructions: false,
				additionalInstructions: "be concise",
			},
		};

		expect(decodeAcpRequest(encodeAcpRequest(request))).toEqual(request);
	});

	test("round-trips atomic session route identity responses", () => {
		const responses: AcpResponse[] = [
			{
				tag: "AcpSessionCreatedResponse",
				val: {
					sessionId: "session-created",
					agentType: "codex",
					processId: "acp-agent-1",
					pid: 42,
					modes: null,
					configOptions: [],
					agentCapabilities: null,
					agentInfo: null,
				},
			},
			{
				tag: "AcpSessionResumedResponse",
				val: {
					sessionId: "session-resumed",
					mode: "fallback",
					agentType: "pi",
					processId: "acp-agent-2",
					pid: 84,
				},
			},
		];

		for (const response of responses) {
			expect(decodeAcpResponse(encodeAcpResponse(response))).toEqual(response);
		}
	});
});
