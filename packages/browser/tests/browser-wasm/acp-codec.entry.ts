// Bundled (esbuild) ACP wire-codec harness for the Chromium ACP round-trip test.
// Builds real wire frames (authenticate + an ACP ext request) using the live
// @rivet-dev/agentos-runtime-core protocol codec + agent-os's generated ACP encoders, drives them
// through the agentos wasm sidecar's pushFrame, and decodes the ACP response. This
// proves a real ACP request/response round-trip through the converged wasm sidecar
// (BrowserAcpExtension -> AcpCore) in the browser.

// The @rivet-dev/agentos-runtime-core codec uses Node's Buffer for the ext-envelope `data` field;
// provide it in the browser so frame decoding works.
import { Buffer as BufferPolyfill } from "buffer";
(globalThis as unknown as { Buffer?: unknown }).Buffer ??= BufferPolyfill;

import { decodeBareProtocolFrame, encodeBareProtocolFrame } from "@rivet-dev/agentos-runtime-core/protocol-frames";
import { SIDECAR_PROTOCOL_SCHEMA } from "@rivet-dev/agentos-runtime-core/protocol-schema";
// agent-os's generated ACP BARE encoders (not exported from the package; imported by path).
import {
	decodeAcpResponse,
	encodeAcpRequest,
} from "../../../core/src/sidecar/agentos-protocol.ts";

const ACP_NS = "dev.rivet.agent-os.acp";
type PushFrame = (frame: Uint8Array) => Uint8Array;

let nextRequestId = 1;

function authenticateFrame(): Uint8Array {
	return encodeBareProtocolFrame({
		frame_type: "request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: nextRequestId++,
		ownership: { scope: "connection", connection_id: "client-hint" },
		payload: {
			type: "authenticate",
			client_name: "agentos-browser-test",
			auth_token: "",
			protocol_version: SIDECAR_PROTOCOL_SCHEMA.version,
			bridge_version: 1,
		},
	} as never);
}

function openSessionFrame(connectionId: string): Uint8Array {
	return encodeBareProtocolFrame({
		frame_type: "request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: nextRequestId++,
		ownership: { scope: "connection", connection_id: connectionId },
		payload: {
			type: "open_session",
			placement: { kind: "shared", pool: null },
		},
	} as never);
}

function initializeVmFrame(connectionId: string, sessionId: string): Uint8Array {
	return encodeBareProtocolFrame({
		frame_type: "request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: nextRequestId++,
		ownership: { scope: "session", connection_id: connectionId, session_id: sessionId },
		payload: { type: "initialize_vm", runtime: "java_script", config: {} },
	} as never);
}

function acpGetSessionStateFrame(
	connectionId: string,
	sidecarSessionId: string,
	vmId: string,
	acpSessionId: string,
): Uint8Array {
	const payload = encodeAcpRequest({
		tag: "AcpGetSessionStateRequest",
		val: { sessionId: acpSessionId },
	});
	return encodeBareProtocolFrame({
		frame_type: "request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: nextRequestId++,
		ownership: {
			scope: "vm",
			connection_id: connectionId,
			session_id: sidecarSessionId,
			vm_id: vmId,
		},
		payload: { type: "ext", envelope: { namespace: ACP_NS, payload } },
	} as never);
}

function decodeResponse(bytes: Uint8Array): {
	frameType: string;
	payloadType?: string;
	acpTag?: string;
	acpMessage?: string;
	connectionId?: string;
	sessionId?: string;
	vmId?: string;
	rejected?: { code?: string; message?: string };
} {
	const frame = decodeBareProtocolFrame(bytes) as {
		frame_type: string;
		payload: Record<string, unknown>;
	};
	const payload = frame.payload;
	const out: ReturnType<typeof decodeResponse> = {
		frameType: frame.frame_type,
		payloadType: payload.type as string,
	};
	out.connectionId = payload.connection_id as string | undefined;
	out.sessionId = payload.session_id as string | undefined;
	out.vmId = payload.vm_id as string | undefined;
	if (payload.type === "ext" || payload.type === "ext_result") {
		const env = payload.envelope as { payload: Uint8Array };
		const acp = decodeAcpResponse(env.payload) as { tag: string; val?: { code?: string; message?: string } };
		out.acpTag = acp.tag;
		out.acpMessage = acp.val?.message;
	} else if (payload.type === "rejected") {
		out.rejected = {
			code: payload.code as string,
			message: payload.message as string,
		};
	}
	return out;
}

(globalThis as unknown as { __acpHarness: unknown }).__acpHarness = {
	runGetSessionStateRoundTrip(pushFrame: PushFrame) {
		const authResp = decodeResponse(pushFrame(authenticateFrame()));
		if (!authResp.connectionId) throw new Error("authentication returned no connection id");
		const openResp = decodeResponse(pushFrame(openSessionFrame(authResp.connectionId)));
		if (!openResp.sessionId) throw new Error("open_session returned no session id");
		const vmResp = decodeResponse(
			pushFrame(initializeVmFrame(authResp.connectionId, openResp.sessionId)),
		);
		if (!vmResp.vmId) throw new Error("initialize_vm returned no VM id");
		const acpResp = decodeResponse(
			pushFrame(
				acpGetSessionStateFrame(
					authResp.connectionId,
					openResp.sessionId,
					vmResp.vmId,
					"does-not-exist",
				),
			),
		);
		return { authResp, openResp, vmResp, acpResp };
	},
};
