import { afterEach, describe, expect, test, vi } from "vitest";
import type { JsonRpcNotification } from "../src/index.js";
import { AgentOs } from "../src/index.js";
import { encodeAcpEvent } from "../src/sidecar/agentos-protocol.js";

// agent-os.ts keeps this namespace as a module-private const; mirror the literal.
const ACP_EXTENSION_NAMESPACE = "dev.rivet.agent-os.acp";

// ---------------------------------------------------------------------------
// AOS-SESS-1 (P1) — colliding adapter sessionId (vectors I.4 / J.4).
//
// THREAT MODEL: the ACP adapter (untrusted upstream agent SDK output) chooses
// the `sessionId` returned in `AcpSessionCreatedResponse`. `createSession`
// (agent-os.ts ~3789) does `this._sessions.set(created.sessionId, session)` at
// ~3851 with NO `has()` guard. If a second createSession returns a sessionId
// that collides with a live session, the existing entry — including all its
// registered event/permission handlers and pending permission replies — is
// silently overwritten and orphaned.
//
// We play an adapter that returns the SAME sessionId twice. A subscriber
// registered against the first session must keep receiving that session's
// events after the second createSession (DENY/isolate the collision). If the
// second create silently overwrites the first entry, the original handler is
// orphaned and the delivered-event assertion FAILS — documenting the break
// (no re-discovery; this is the in-scope assertion of the gap).
// ---------------------------------------------------------------------------

const COLLIDING_SESSION_ID = "mock-session-1";

function cannedCreatedResponse(sessionId: string) {
	return {
		tag: "AcpSessionCreatedResponse" as const,
		val: {
			sessionId,
			pid: null,
			modes: null,
			configOptions: [] as readonly string[],
			agentCapabilities: null,
			agentInfo: null,
		},
	};
}

function cannedStateResponse(sessionId: string) {
	return {
		tag: "AcpSessionStateResponse" as const,
		val: {
			sessionId,
			agentType: "mock",
			processId: "",
			pid: null,
			closed: false,
			exitCode: null,
			modes: null,
			configOptions: [] as readonly string[],
			agentCapabilities: null,
			agentInfo: null,
		},
	};
}

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

describe("colliding adapter sessionId isolation (I.4 / J.4)", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("a second createSession returning a colliding sessionId must not orphan the first session's handlers", async () => {
		vm = await AgentOs.create({});

		const internal = vm as unknown as {
			_resolveAgentConfig(t: string): unknown;
			_resolveAdapterBin(p: string): string;
			_sendAcpRequest(req: { tag: string }): Promise<unknown>;
		};

		// The adapter (untrusted) always reports the same sessionId.
		vi.spyOn(internal, "_resolveAgentConfig").mockReturnValue({
			acpAdapter: "@mock/adapter",
			agentPackage: "@mock/agent",
		});
		vi.spyOn(internal, "_resolveAdapterBin").mockReturnValue(
			"/root/node_modules/@mock/adapter/bin.js",
		);
		vi.spyOn(internal, "_sendAcpRequest").mockImplementation(
			async (req: { tag: string }) => {
				if (req.tag === "AcpCreateSessionRequest") {
					return cannedCreatedResponse(COLLIDING_SESSION_ID);
				}
				if (req.tag === "AcpGetSessionStateRequest") {
					return cannedStateResponse(COLLIDING_SESSION_ID);
				}
				throw new Error(`unexpected acp request ${req.tag}`);
			},
		);

		const first = await vm.createSession("mock");
		expect(first.sessionId).toBe(COLLIDING_SESSION_ID);

		// Subscribe a handler against the FIRST session.
		const firstEvents: JsonRpcNotification[] = [];
		vm.onSessionEvent(first.sessionId, (n) => firstEvents.push(n));

		// The adapter creates a "second" session with a colliding id.
		const second = await vm.createSession("mock").then(
			(r) => ({ ok: true as const, r }),
			(e) => ({ ok: false as const, e }),
		);

		// Now drive a session/update for that id and see whether the first
		// subscriber still receives it.
		const payload = encodeAcpEvent({
			tag: "AcpSessionEvent",
			val: {
				sessionId: COLLIDING_SESSION_ID,
				notification: sessionUpdateNotification("post-collision"),
			},
		});
		(
			vm as unknown as {
				_handleAcpExtEvent(env: {
					namespace: string;
					payload: Uint8Array;
				}): void;
			}
		)._handleAcpExtEvent({ namespace: ACP_EXTENSION_NAMESPACE, payload });

		// DENY / isolate: either the colliding create was rejected (so the first
		// session + its handler survive untouched), or — if it was accepted — the
		// original handler must NOT have been orphaned. A silent overwrite that
		// drops the first session's handler set is the vulnerability and fails here.
		if (!second.ok) {
			// Rejected collision is an acceptable defensive outcome.
			expect(firstEvents).toHaveLength(1);
		} else {
			// Accepted: the original subscriber must still be wired up.
			expect(firstEvents).toHaveLength(1);
		}
	});
});
