import { afterEach, describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/index.js";

// ---------------------------------------------------------------------------
// AOS-SESS-3 (P2) — cross-session permission-reply confusion (vectors J.4 / F.3).
//
// Permission correlation is sidecar-owned now. The client must preserve both the
// public session id and AgentOS request id exactly; it must not collapse requests
// into a client-side map keyed only by the adapter's JSON-RPC id.
// ---------------------------------------------------------------------------

describe("cross-session permission-reply confusion (J.4 / F.3)", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("respondPermission preserves session scope for duplicate request ids", async () => {
		vm = await AgentOs.create({});
		const internal = vm as unknown as {
			_sendAcpRequest(request: unknown): Promise<unknown>;
		};
		const send = vi.spyOn(internal, "_sendAcpRequest").mockResolvedValue({
			tag: "AcpRespondPermissionResponse",
			val: { status: "accepted", reason: null },
		} as never);

		await vm.respondPermission({
			sessionId: "session-A",
			requestId: "1",
			optionId: "allow-a",
		});
		await vm.respondPermission({
			sessionId: "session-B",
			requestId: "1",
			optionId: "allow-b",
		});

		expect(send).toHaveBeenNthCalledWith(1, {
			tag: "AcpRespondPermissionRequest",
			val: {
				sessionId: "session-A",
				requestId: "1",
				optionId: "allow-a",
			},
		});
		expect(send).toHaveBeenNthCalledWith(2, {
			tag: "AcpRespondPermissionRequest",
			val: {
				sessionId: "session-B",
				requestId: "1",
				optionId: "allow-b",
			},
		});
	});
});
