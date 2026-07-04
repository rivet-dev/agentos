import { describe, expect, test } from "vitest";
import {
	type AcpRequest,
	AcpRuntimeKind,
	decodeAcpRequest,
	encodeAcpRequest,
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
				env: new Map([["SECURE_EXEC_KEEP_STDIN_OPEN", "1"]]),
				protocolVersion: 1,
				clientCapabilities: "{}",
				mcpServers: "{}",
				skipOsInstructions: false,
				additionalInstructions: "be concise",
			},
		};

		expect(decodeAcpRequest(encodeAcpRequest(request))).toEqual(request);
	});
});
