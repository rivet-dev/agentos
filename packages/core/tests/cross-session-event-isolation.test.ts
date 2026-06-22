import { afterEach, describe, expect, test } from "vitest";
import type { JsonRpcNotification } from "../src/index.js";
import { AgentOs } from "../src/index.js";
import { encodeAcpEvent } from "../src/sidecar/agentos-protocol.js";

// agent-os.ts keeps this namespace as a module-private const; mirror the literal
// here (the routing in `_handleAcpExtEvent` compares against exactly this value).
const ACP_EXTENSION_NAMESPACE = "dev.rivet.agent-os.acp";

// ---------------------------------------------------------------------------
// AOS-SESS-2 (P2) — cross-session ACP event isolation (vector J.4).
//
// THREAT MODEL: the sidecar event stream is untrusted to the extent that an
// `AcpSessionEvent` carries a `sessionId` chosen by the wire. `_handleAcpExtEvent`
// (agent-os.ts ~3486) decodes the envelope and routes the notification to the
// session named by `event.val.sessionId`. We play an event stream that stamps a
// `session/update` for session B and assert it is delivered ONLY to B's
// subscribers — never leaked to a subscriber that registered on A.
//
// This is a regression guard: routing is keyed strictly on the decoded
// sessionId, so A's handler must stay empty. A FAIL here would mean an event
// for one session bleeds into another session's update stream.
// ---------------------------------------------------------------------------

function sessionUpdateNotification(text: string): string {
	return JSON.stringify({
		jsonrpc: "2.0",
		method: "session/update",
		params: {
			update: {
				sessionUpdate: "agent_message_chunk",
				content: { type: "text", text },
			},
		},
	});
}

function injectSession(vm: AgentOs, sessionId: string): void {
	const sessions = (vm as unknown as { _sessions: Map<string, unknown> })
		._sessions;
	sessions.set(sessionId, {
		sessionId,
		agentType: "mock",
		processId: "",
		pid: null,
		closed: false,
		modes: null,
		configOptions: [],
		capabilities: {},
		agentInfo: null,
		eventHandlers: new Set(),
		permissionHandlers: new Set(),
		configOverrides: new Map(),
		pendingPermissionReplies: new Map(),
	});
}

describe("cross-session ACP event isolation (J.4)", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("session/update stamped for B is not delivered to A's event handlers", async () => {
		// Minimal VM — we drive the private event router directly with forged
		// envelopes; no in-VM guest code is spawned.
		vm = await AgentOs.create({});

		injectSession(vm, "session-A");
		injectSession(vm, "session-B");

		const aEvents: JsonRpcNotification[] = [];
		const bEvents: JsonRpcNotification[] = [];
		vm.onSessionEvent("session-A", (n) => aEvents.push(n));
		vm.onSessionEvent("session-B", (n) => bEvents.push(n));

		const payload = encodeAcpEvent({
			tag: "AcpSessionEvent",
			val: {
				sessionId: "session-B",
				notification: sessionUpdateNotification("for B only"),
			},
		});

		(
			vm as unknown as {
				_handleAcpExtEvent(env: {
					namespace: string;
					payload: Uint8Array;
				}): void;
			}
		)._handleAcpExtEvent({
			namespace: ACP_EXTENSION_NAMESPACE,
			payload,
		});

		// The attack: an event for B must NOT leak into A's stream.
		expect(aEvents).toHaveLength(0);
		expect(bEvents).toHaveLength(1);
		expect(bEvents[0]?.method).toBe("session/update");
	});
});
