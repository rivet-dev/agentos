import { afterEach, describe, expect, test } from "vitest";
import type { PermissionReply } from "../src/index.js";
import { AgentOs } from "../src/index.js";

// ---------------------------------------------------------------------------
// AOS-SESS-3 (P2) — cross-session permission-reply confusion (vectors J.4 / F.3).
//
// THREAT MODEL: permission ids are minted per-adapter as `String(request.id)`
// (agent-os.ts ~4172), so two concurrent sessions can each have a pending
// permission keyed under the SAME id (e.g. "1"). `respondPermission(sessionId,
// permissionId, reply)` (agent-os.ts ~4748) resolves the pending reply found in
// THAT session's `pendingPermissionReplies` map. We assert that replying to
// session A's permission "1" resolves only A's promise and leaves B's identically
// numbered permission still pending — a cross-session reply must not satisfy
// another session's prompt (which would let one tenant auto-approve another's
// permission request).
// ---------------------------------------------------------------------------

interface PendingReply {
	resolve: (reply: PermissionReply) => void;
	reject: (error: Error) => void;
	timer: ReturnType<typeof setTimeout>;
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
		pendingPermissionReplies: new Map<string, PendingReply>(),
	});
}

function addPendingPermission(
	vm: AgentOs,
	sessionId: string,
	permissionId: string,
): Promise<PermissionReply> {
	const sessions = (
		vm as unknown as {
			_sessions: Map<
				string,
				{ pendingPermissionReplies: Map<string, PendingReply> }
			>;
		}
	)._sessions;
	const session = sessions.get(sessionId);
	if (!session) {
		throw new Error(`no session ${sessionId}`);
	}
	return new Promise<PermissionReply>((resolve, reject) => {
		const timer = setTimeout(() => {
			session.pendingPermissionReplies.delete(permissionId);
			reject(new Error(`timed out: ${sessionId}/${permissionId}`));
		}, 5_000);
		session.pendingPermissionReplies.set(permissionId, {
			resolve,
			reject,
			timer,
		});
	});
}

describe("cross-session permission-reply confusion (J.4 / F.3)", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("respondPermission(A,'1') does not resolve B's same-numbered pending reply", async () => {
		vm = await AgentOs.create({});

		injectSession(vm, "session-A");
		injectSession(vm, "session-B");

		// Both sessions have an in-flight permission prompt keyed under "1".
		const aPending = addPendingPermission(vm, "session-A", "1");
		const bPending = addPendingPermission(vm, "session-B", "1");

		let bResolved = false;
		void bPending.then(
			() => {
				bResolved = true;
			},
			() => {
				/* timeout/reject ignored for the assertion below */
			},
		);

		// Reply to A's prompt only.
		await vm.respondPermission("session-A", "1", "always");

		// A's promise must resolve with the supplied reply.
		await expect(aPending).resolves.toBe("always");

		// Give any erroneous cross-session resolution a microtask/tick to surface.
		await new Promise((r) => setTimeout(r, 0));

		// The attack: B's identically numbered permission must STILL be pending.
		expect(bResolved).toBe(false);

		const bMap = (
			vm as unknown as {
				_sessions: Map<
					string,
					{ pendingPermissionReplies: Map<string, PendingReply> }
				>;
			}
		)._sessions.get("session-B")?.pendingPermissionReplies;
		expect(bMap?.has("1")).toBe(true);

		// Clean up B's still-pending timer so the test exits promptly.
		const bEntry = bMap?.get("1");
		if (bEntry) {
			clearTimeout(bEntry.timer);
			bEntry.resolve("reject");
		}
	});
});
