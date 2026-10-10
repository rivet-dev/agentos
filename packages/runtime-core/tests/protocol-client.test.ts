import { Duplex, PassThrough } from "node:stream";
import { describe, expect, it, vi } from "vitest";
import type { FrameTransport } from "../src/frame-stream.js";
import { encodeLengthPrefixedPayload } from "../src/framing.js";
import { SidecarProtocolClient } from "../src/protocol-client.js";
import {
	encodeProtocolFramePayload,
	type LiveEventFrame,
	type LiveProtocolFrame,
	type LiveResponseFrame,
	type LiveSidecarRequestFrame,
} from "../src/protocol-frames.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";
import {
	SidecarRejectedError,
	SidecarVmRequestHandlerLimit,
} from "../src/sidecar-errors.js";

const ownership = {
	scope: "connection" as const,
	connection_id: "conn",
};

function createClient() {
	const stdin = new PassThrough();
	const stdout = new PassThrough();
	const control = new TestControlStream();
	const client = new SidecarProtocolClient({
		stdin,
		stdout,
		control,
		eventBufferCapacity: 8,
		payloadCodec: "json",
		stderrText: () => "stderr",
	});
	return { stdin, stdout, control, client };
}

class TestControlStream extends Duplex {
	readonly written = new PassThrough();

	_read(): void {}

	_write(
		chunk: Buffer | string,
		encoding: BufferEncoding,
		callback: (error?: Error | null) => void,
	): void {
		this.written.write(chunk, encoding, callback);
	}

	receive(bytes: Uint8Array): void {
		this.push(bytes);
	}
}

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
		return () => {
			this.frameListeners.delete(handler);
		};
	}

	onError(handler: (error: Error) => void): () => void {
		this.errorListeners.add(handler);
		return () => {
			this.errorListeners.delete(handler);
		};
	}

	onEnd(handler: () => void): () => void {
		this.endListeners.add(handler);
		return () => {
			this.endListeners.delete(handler);
		};
	}

	async writeFrame(frame: LiveProtocolFrame): Promise<void> {
		this.writes.push(frame);
	}

	emitFrame(
		frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
	): void {
		for (const listener of this.frameListeners) {
			listener(frame);
		}
	}

	dispose(): void {
		this.frameListeners.clear();
		this.errorListeners.clear();
		this.endListeners.clear();
	}
}

function readWrittenFrame(stdin: PassThrough): Promise<unknown> {
	return new Promise((resolve) => {
		stdin.once("data", (chunk: Buffer) => {
			const payloadLength = chunk.readUInt32BE(0);
			resolve(
				JSON.parse(chunk.subarray(4, 4 + payloadLength).toString("utf8")),
			);
		});
	});
}

function writeIncomingFrame(
	stdout: PassThrough,
	frame: LiveProtocolFrame,
): void {
	stdout.write(
		encodeLengthPrefixedPayload(encodeProtocolFramePayload(frame, "json")),
	);
}

function writeIncomingControlFrame(
	control: TestControlStream,
	frame: LiveProtocolFrame,
): void {
	control.receive(
		encodeLengthPrefixedPayload(encodeProtocolFramePayload(frame, "json")),
	);
}

describe("sidecar protocol client", () => {
	it("bounds VM callback registrations, warns near capacity, and reuses released slots", () => {
		const client = new SidecarProtocolClient({
			frameTransport: new MemoryFrameTransport(),
			eventBufferCapacity: 2,
			maxVmRequestHandlers: 5,
		});
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
		const handler = async () => ({
			type: "host_callback_result" as const,
			invocation_id: "invocation",
			result: null,
		});
		try {
			for (let i = 0; i < 4; i++)
				client.setSidecarRequestHandler(handler, `vm-${i}`);
			expect(warn).toHaveBeenCalledOnce();
			expect(warn.mock.calls[0][0]).toContain("maxVmRequestHandlers");
			client.setSidecarRequestHandler(handler, "vm-4");
			expect(() => client.setSidecarRequestHandler(handler, "vm-5")).toThrow(
				SidecarVmRequestHandlerLimit,
			);
			// Replacing an existing VM does not consume another slot.
			client.setSidecarRequestHandler(handler, "vm-0");
			client.setSidecarRequestHandler(null, "vm-1");
			client.setSidecarRequestHandler(handler, "vm-5");
		} finally {
			warn.mockRestore();
			client.dispose();
		}
	});

	it("rejects new callback registrations after normal protocol disposal", () => {
		const client = new SidecarProtocolClient({
			frameTransport: new MemoryFrameTransport(),
			eventBufferCapacity: 2,
		});
		client.dispose();
		expect(() =>
			client.setSidecarRequestHandler(
				async () => ({
					type: "host_callback_result",
					invocation_id: "invocation",
					result: null,
				}),
				"vm",
			),
		).toThrow("sidecar protocol client disposed");
		// Cleanup remains possible after transport closure.
		client.setSidecarRequestHandler(null, "vm");
	});

	it("preserves structured resource-limit rejection metadata", async () => {
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 2,
			payloadCodec: "json",
		});
		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});
		await expect.poll(() => frameTransport.writes.length).toBe(1);
		frameTransport.emitFrame({
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 1,
			ownership,
			payload: {
				type: "rejected",
				code: "ERR_AGENTOS_RESOURCE_LIMIT",
				message: "handle command bytes exceeded",
				limit_name: "handleCommandBytes",
				configured_limit: 4096,
				current_usage: 3072,
				requested: 2048,
				unit: "bytes",
				scope: "vm",
				vm_id: "vm-1",
				session_generation: 3,
				capability_id: 11,
				operation: "socket.write",
				configuration_path: "limits.reactor.maxHandleCommandBytes",
				retryable: true,
				errno: "EAGAIN",
			},
		});

		const error = await response.catch((cause: unknown) => cause);
		expect(error).toBeInstanceOf(SidecarRejectedError);
		expect((error as SidecarRejectedError).detail).toMatchObject({
			limit_name: "handleCommandBytes",
			configured_limit: 4096,
			configuration_path: "limits.reactor.maxHandleCommandBytes",
			retryable: true,
			errno: "EAGAIN",
		});
		client.dispose();
	});

	it("sends host request frames and correlates responses", async () => {
		const { stdin, control, client } = createClient();
		const written = readWrittenFrame(stdin);

		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});

		await expect(written).resolves.toMatchObject({
			frame_type: "request",
			request_id: 1,
			payload: { type: "create_layer" },
		});

		writeIncomingControlFrame(control, {
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 1,
			ownership,
			payload: { type: "layer_created", layer_id: "layer" },
		});

		await expect(response).resolves.toMatchObject({
			frame_type: "response",
			request_id: 1,
			payload: { type: "layer_created", layer_id: "layer" },
		});
		client.dispose();
	});

	it("can run over an injected non-stdio frame transport", async () => {
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 8,
			payloadCodec: "json",
			stderrText: () => "stderr",
		});

		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});

		await expect.poll(() => frameTransport.writes.length).toBe(1);
		expect(frameTransport.writes[0]).toMatchObject({
			frame_type: "request",
			request_id: 1,
			payload: { type: "create_layer" },
		});

		frameTransport.emitFrame({
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 1,
			ownership,
			payload: { type: "layer_created", layer_id: "layer" },
		});

		await expect(response).resolves.toMatchObject({
			frame_type: "response",
			request_id: 1,
			payload: { type: "layer_created", layer_id: "layer" },
		});
		client.dispose();
	});

	it("delivers event frames to waiters", async () => {
		const { stdout, client } = createClient();
		const event = client.waitForEvent({
			type: "structured",
			name: "ready",
		});

		writeIncomingFrame(stdout, {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: {
				type: "structured",
				name: "ready",
				detail: { ok: "true" },
			},
		});

		await expect(event).resolves.toMatchObject({
			frame_type: "event",
			payload: {
				type: "structured",
				name: "ready",
				detail: { ok: "true" },
			},
		});
		client.dispose();
	});

	it("swallows heartbeat events before waiters and the bounded buffer", async () => {
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 2,
			payloadCodec: "json",
			stderrText: () => "stderr",
		});
		const heartbeat = (): LiveEventFrame => ({
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "structured", name: "heartbeat", detail: {} },
		});

		// Far more heartbeats than the buffer capacity: if any were buffered,
		// the overflow would fail the client permanently and the later request
		// below would reject.
		for (let i = 0; i < 8; i += 1) {
			frameTransport.emitFrame(heartbeat());
		}

		// A predicate waiter that would match anything must not see heartbeats.
		let sawHeartbeat = false;
		const waiter = client.waitForEvent((event) => {
			if (
				event.payload.type === "structured" &&
				event.payload.name === "heartbeat"
			) {
				sawHeartbeat = true;
			}
			return event.payload.type === "vm_lifecycle";
		});
		frameTransport.emitFrame(heartbeat());
		frameTransport.emitFrame({
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "vm_lifecycle", state: "ready" },
		});

		await expect(waiter).resolves.toMatchObject({
			payload: { type: "vm_lifecycle", state: "ready" },
		});
		expect(sawHeartbeat).toBe(false);
		client.dispose();
	});

	it("rejects in-flight requests when the silence watchdog fires", async () => {
		const frameTransport = new MemoryFrameTransport();
		let expired = 0;
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 8,
			payloadCodec: "json",
			stderrText: () => "sidecar stderr tail",
			silenceTimeoutMs: 50,
			onSilenceExpired: () => {
				expired += 1;
			},
		});

		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});

		await expect(response).rejects.toThrow(
			/sidecar unresponsive: no protocol frames or heartbeats for \d+ms/,
		);
		expect(expired).toBe(1);
		// The client is failed permanently: later requests reject immediately.
		await expect(
			client.sendRequest({ ownership, payload: { type: "create_layer" } }),
		).rejects.toThrow(/sidecar unresponsive/);
		client.dispose();
	});

	it("inbound frames reset the silence watchdog", async () => {
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 8,
			payloadCodec: "json",
			stderrText: () => "stderr",
			silenceTimeoutMs: 120,
		});

		// Keep beating for well past the silence window; the client must stay
		// healthy because every heartbeat resets the clock.
		for (let i = 0; i < 6; i += 1) {
			await new Promise((resolve) => setTimeout(resolve, 40));
			frameTransport.emitFrame({
				frame_type: "event",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				ownership,
				payload: { type: "structured", name: "heartbeat", detail: {} },
			});
		}

		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});
		await expect.poll(() => frameTransport.writes.length).toBe(1);
		frameTransport.emitFrame({
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 1,
			ownership,
			payload: { type: "layer_created", layer_id: "layer" },
		});
		await expect(response).resolves.toMatchObject({
			payload: { type: "layer_created", layer_id: "layer" },
		});
		client.dispose();
	});

	it("writes sidecar request handler responses", async () => {
		const { control, client } = createClient();
		const written = readWrittenFrame(control.written);
		client.setSidecarRequestHandler(async () => ({
			type: "host_callback_result",
			invocation_id: "invocation",
			result: { ok: true },
		}));

		writeIncomingControlFrame(control, {
			frame_type: "sidecar_request",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 7,
			ownership,
			payload: {
				type: "host_callback",
				invocation_id: "invocation",
				callback_key: "tool",
				input: {},
				timeout_ms: 1000,
			},
		});

		await expect(written).resolves.toMatchObject({
			frame_type: "sidecar_response",
			request_id: 7,
			payload: {
				type: "host_callback_result",
				invocation_id: "invocation",
				result: { ok: true },
			},
		});
		client.dispose();
	});

	it.each([
		"host_callback",
		"js_bridge_call",
	] as const)("routes %s by VM without borrowing a sibling or global handler", async (type) => {
		const bridge = type === "js_bridge_call";
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 8,
			payloadCodec: "json",
		});
		const calls: string[] = [];
		for (const vmId of ["a", "b"]) {
			client.setSidecarRequestHandler(async (request) => {
				calls.push(vmId);
				if (request.payload.type === "js_bridge_call") {
					return {
						type: "js_bridge_result",
						call_id: request.payload.call_id,
						result: vmId,
					};
				}
				return {
					type: "host_callback_result",
					invocation_id: "invoke",
					result: vmId,
				};
			}, vmId);
		}
		client.setSidecarRequestHandler(async () => {
			calls.push("global");
			return {
				type: "host_callback_result",
				invocation_id: "invoke",
				result: "global",
			};
		});
		const emit = (vmId: string, requestId: number, bridge = false) =>
			frameTransport.emitFrame({
				frame_type: "sidecar_request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: requestId,
				ownership: {
					scope: "vm",
					connection_id: "conn",
					session_id: "session",
					vm_id: vmId,
				},
				payload: bridge
					? {
							type: "js_bridge_call",
							call_id: "bridge",
							mount_id: "mount",
							operation: "read_file",
							args: [],
						}
					: {
							type: "host_callback",
							invocation_id: "invoke",
							callback_key: "tool",
							input: {},
							timeout_ms: 1000,
						},
			});
		const response = (id: number) =>
			frameTransport.writes.find(
				(frame) => "request_id" in frame && frame.request_id === id,
			);
		try {
			emit("a", -1, bridge);
			emit("b", -2, bridge);
			emit("unregistered", -3, bridge);
			await expect.poll(() => frameTransport.writes.length).toBe(3);
			expect(calls).toEqual(["a", "b"]);
			expect(response(-1)).toMatchObject({
				request_id: -1,
				ownership: { scope: "vm", vm_id: "a" },
				payload: { result: "a" },
			});
			expect(response(-2)).toMatchObject({
				request_id: -2,
				payload: { result: "b" },
			});
			expect(response(-3)).toMatchObject({
				request_id: -3,
				payload: {
					error: expect.stringContaining("no sidecar request handler"),
				},
			});
			client.setSidecarRequestHandler(null, "b");
			client.setSidecarRequestHandler(null, "disabled");
			emit("b", -4, bridge);
			emit("a", -5, bridge);
			await expect.poll(() => frameTransport.writes.length).toBe(5);
			expect(calls).toEqual(["a", "b", "a"]);
			expect(response(-4)).toMatchObject({
				request_id: -4,
				payload: { error: expect.any(String) },
			});
			expect(response(-5)).toMatchObject({
				request_id: -5,
				payload: { result: "a" },
			});
		} finally {
			client.dispose();
		}
	});

	it.each([
		"connection",
		"session",
	] as const)("keeps the global handler for %s requests", async (scope) => {
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 8,
			payloadCodec: "json",
		});
		client.setSidecarRequestHandler(async () => ({
			type: "host_callback_result",
			invocation_id: "invoke",
			result: "global",
		}));
		client.setSidecarRequestHandler(
			async () => ({
				type: "host_callback_result",
				invocation_id: "invoke",
				result: "vm",
			}),
			"a",
		);
		try {
			frameTransport.emitFrame({
				frame_type: "sidecar_request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: -1,
				ownership:
					scope === "connection"
						? ownership
						: { scope, connection_id: "conn", session_id: "session" },
				payload: {
					type: "host_callback",
					invocation_id: "invoke",
					callback_key: "tool",
					input: {},
					timeout_ms: 1000,
				},
			});
			await expect.poll(() => frameTransport.writes.length).toBe(1);
			expect(frameTransport.writes[0]).toMatchObject({
				payload: { result: "global" },
			});
		} finally {
			client.dispose();
		}
	});

	it("writes typed shutdown control on fd3 without touching fd0", async () => {
		const { stdin, control, client } = createClient();
		const written = readWrittenFrame(control.written);

		await client.shutdown("test complete");

		await expect(written).resolves.toEqual({
			frame_type: "control",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			payload: { type: "shutdown", reason: "test complete" },
		});
		expect(stdin.readableLength).toBe(0);
		client.dispose();
	});

	it("rejects response frames arriving on the ordinary event lane", async () => {
		const { stdout, client } = createClient();
		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});

		writeIncomingFrame(stdout, {
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 1,
			ownership,
			payload: { type: "layer_created", layer_id: "wrong-lane" },
		});

		await expect(response).rejects.toThrow(
			"sidecar frame response is not valid on the ordinary protocol lane",
		);
		client.dispose();
	});

	it("rejects application events arriving on the control lane", async () => {
		const { stdout, control, client } = createClient();
		let deliveredEvents = 0;
		client.onEvent(() => {
			deliveredEvents += 1;
		});
		const event = client.waitForEvent({ type: "vm_lifecycle" });

		writeIncomingControlFrame(control, {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "vm_lifecycle", state: "ready" },
		});

		await expect(event).rejects.toThrow(
			"sidecar frame event is not valid on the control protocol lane",
		);
		writeIncomingFrame(stdout, {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "vm_lifecycle", state: "ready" },
		});
		expect(deliveredEvents).toBe(0);
		client.dispose();
	});

	it("accepts control-lane heartbeats without exposing them", async () => {
		const { stdout, control, client } = createClient();
		const event = client.waitForEvent({ type: "vm_lifecycle" });

		writeIncomingControlFrame(control, {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "structured", name: "heartbeat", detail: {} },
		});
		writeIncomingFrame(stdout, {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "vm_lifecycle", state: "ready" },
		});

		await expect(event).resolves.toMatchObject({
			payload: { type: "vm_lifecycle", state: "ready" },
		});
		client.dispose();
	});

	it("treats control lane EOF as terminal for ordinary requests", async () => {
		const { control, client } = createClient();
		const response = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});

		control.push(null);

		await expect(response).rejects.toThrow("sidecar protocol stream ended");
		client.dispose();
	});
});
