import { describe, expect, it } from "vitest";
import type { FrameTransport } from "../src/frame-stream.js";
import type { LiveOwnershipScope } from "../src/ownership.js";
import type {
	LiveEventFrame,
	LiveProtocolFrame,
	LiveResponseFrame,
	LiveSidecarRequestFrame,
} from "../src/protocol-frames.js";
import { SidecarProtocolClient } from "../src/protocol-client.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";

class MemoryFrameTransport
	implements
		FrameTransport<
			LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
			LiveProtocolFrame
		>
{
	readonly writes: LiveProtocolFrame[] = [];
	private readonly frameListeners = new Set<
		(
			frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
		) => void
	>();
	private readonly errorListeners = new Set<(error: Error) => void>();
	private readonly endListeners = new Set<() => void>();

	onFrame(
		handler: (
			frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
		) => void,
	): () => void {
		this.frameListeners.add(handler);
		return () => this.frameListeners.delete(handler);
	}

	onError(handler: (error: Error) => void): () => void {
		this.errorListeners.add(handler);
		return () => this.errorListeners.delete(handler);
	}

	onEnd(handler: () => void): () => void {
		this.endListeners.add(handler);
		return () => this.endListeners.delete(handler);
	}

	async writeFrame(frame: LiveProtocolFrame): Promise<void> {
		this.writes.push(frame);
	}

	emitFrame(
		frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
	): void {
		for (const listener of this.frameListeners) listener(frame);
	}

	dispose(): void {
		this.frameListeners.clear();
		this.errorListeners.clear();
		this.endListeners.clear();
	}
}

const vmA: LiveOwnershipScope = {
	scope: "vm",
	connection_id: "connection",
	session_id: "session",
	vm_id: "vm-a",
};
const vmB: LiveOwnershipScope = {
	scope: "vm",
	connection_id: "connection",
	session_id: "session",
	vm_id: "vm-b",
};

function sidecarRequest(
	requestId: number,
	ownership: LiveOwnershipScope,
): LiveSidecarRequestFrame {
	return {
		frame_type: "sidecar_request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: requestId,
		ownership,
		payload: {
			type: "host_callback",
			invocation_id: `invocation-${requestId}`,
			callback_key: "same-tool-name",
			input: {},
			timeout_ms: 1_000,
		},
	};
}

function jsBridgeRequest(
	requestId: number,
	ownership: LiveOwnershipScope,
): LiveSidecarRequestFrame {
	return {
		frame_type: "sidecar_request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: requestId,
		ownership,
		payload: {
			type: "js_bridge_call",
			call_id: `bridge-${requestId}`,
			mount_id: "/same-path",
			operation: "read_file",
			args: { path: "/same-path/value" },
		},
	};
}

function acpRequest(
	requestId: number,
	ownership: LiveOwnershipScope,
): LiveSidecarRequestFrame {
	return {
		frame_type: "sidecar_request",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		request_id: requestId,
		ownership,
		payload: {
			type: "ext",
			envelope: {
				namespace: "agentos.acp.v1",
				payload: new Uint8Array([requestId]),
			},
		},
	};
}

function cronEvent(ownership: LiveOwnershipScope): LiveEventFrame {
	return {
		frame_type: "event",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		ownership,
		payload: {
			type: "cron_dispatch",
			dispatch: {
				alarm: { generation: 1 },
				runs: [],
				events: [],
			},
		},
	};
}

function extensionEvent(ownership: LiveOwnershipScope): LiveEventFrame {
	return {
		frame_type: "event",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		ownership,
		payload: {
			type: "ext",
			envelope: {
				namespace: "agentos.acp.v1",
				payload: new Uint8Array([1]),
			},
		},
	};
}

function warningEvent(ownership: LiveOwnershipScope): LiveEventFrame {
	return {
		frame_type: "event",
		schema: SIDECAR_PROTOCOL_SCHEMA,
		ownership,
		payload: {
			type: "structured",
			name: "limit_warning",
			detail: { limit: "vm_open_fds" },
		},
	};
}

describe("shared sidecar ownership routing", () => {
	it("routes requests and events only to their registered VM", async () => {
		const transport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport: transport,
			eventBufferCapacity: 8,
		});
		const handled: string[] = [];
		const events: string[] = [];
		const handlerFor =
			(owner: "a" | "b") => async (request: LiveSidecarRequestFrame) => {
				handled.push(
					`${owner}:${request.payload.type}:${request.ownership.scope === "vm" && request.ownership.vm_id}`,
				);
				switch (request.payload.type) {
					case "host_callback":
						return {
							type: "host_callback_result" as const,
							invocation_id: request.payload.invocation_id,
							result: owner,
						};
					case "js_bridge_call":
						return {
							type: "js_bridge_result" as const,
							call_id: request.payload.call_id,
							result: owner,
						};
					case "ext":
						return {
							type: "ext_result" as const,
							envelope: {
								namespace: request.payload.envelope.namespace,
								payload: new TextEncoder().encode(owner),
							},
						};
				}
			};

		client.registerSidecarRequestHandler(vmA, handlerFor("a"));
		const unregisterB = client.registerSidecarRequestHandler(
			vmB,
			handlerFor("b"),
		);
		client.onEvent(() => events.push("a"), vmA);
		client.onEvent(() => events.push("b"), vmB);

		transport.emitFrame(sidecarRequest(1, vmA));
		transport.emitFrame(sidecarRequest(2, vmB));
		transport.emitFrame(jsBridgeRequest(3, vmA));
		transport.emitFrame(jsBridgeRequest(4, vmB));
		transport.emitFrame(acpRequest(5, vmA));
		transport.emitFrame(acpRequest(6, vmB));
		transport.emitFrame(cronEvent(vmA));
		transport.emitFrame(cronEvent(vmB));
		transport.emitFrame(extensionEvent(vmA));
		transport.emitFrame(extensionEvent(vmB));
		transport.emitFrame(warningEvent(vmA));
		transport.emitFrame(warningEvent(vmB));

		await expect.poll(() => transport.writes.length).toBe(6);
		expect(handled).toEqual([
			"a:host_callback:vm-a",
			"b:host_callback:vm-b",
			"a:js_bridge_call:vm-a",
			"b:js_bridge_call:vm-b",
			"a:ext:vm-a",
			"b:ext:vm-b",
		]);
		expect(events).toEqual(["a", "b", "a", "b", "a", "b"]);
		expect(transport.writes).toMatchObject([
			{ request_id: 1, ownership: vmA, payload: { result: "a" } },
			{ request_id: 2, ownership: vmB, payload: { result: "b" } },
			{ request_id: 3, ownership: vmA, payload: { result: "a" } },
			{ request_id: 4, ownership: vmB, payload: { result: "b" } },
			{ request_id: 5, ownership: vmA },
			{ request_id: 6, ownership: vmB },
		]);

		unregisterB();
		transport.emitFrame(sidecarRequest(7, vmB));
		await expect.poll(() => transport.writes.length).toBe(7);
		expect(handled).toHaveLength(6);
		expect(transport.writes[6]).toMatchObject({
			request_id: 7,
			ownership: vmB,
			payload: { error: expect.stringContaining("no sidecar request handler") },
		});

		client.dispose();
	});
});
