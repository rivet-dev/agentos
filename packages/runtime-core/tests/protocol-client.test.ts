import { PassThrough } from "node:stream";
import { describe, expect, it } from "vitest";
import type { FrameTransport } from "../src/frame-stream.js";
import { encodeLengthPrefixedPayload } from "../src/framing.js";
import {
	encodeProtocolFramePayload,
	type LiveProtocolFrame,
	type LiveResponseFrame,
	type LiveEventFrame,
	type LiveSidecarRequestFrame,
} from "../src/protocol-frames.js";
import { SidecarProtocolClient } from "../src/protocol-client.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";

const ownership = {
	scope: "connection" as const,
	connection_id: "conn",
};

function createClient() {
	const stdin = new PassThrough();
	const stdout = new PassThrough();
	const client = new SidecarProtocolClient({
		stdin,
		stdout,
		eventBufferCapacity: 8,
		payloadCodec: "json",
		stderrText: () => "stderr",
	});
	return { stdin, stdout, client };
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
		(frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame) => void
	>();
	private readonly errorListeners = new Set<(error: Error) => void>();
	private readonly endListeners = new Set<() => void>();

	onFrame(
		handler: (frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame) => void,
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

	emitFrame(frame: LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame): void {
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

function writeIncomingFrame(stdout: PassThrough, frame: LiveProtocolFrame): void {
	stdout.write(
		encodeLengthPrefixedPayload(encodeProtocolFramePayload(frame, "json")),
	);
}

describe("sidecar protocol client", () => {
	it("sends host request frames and correlates responses", async () => {
		const { stdin, stdout, client } = createClient();
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

		writeIncomingFrame(stdout, {
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

	it("delivers filesystem.changed to listeners without filling the bounded buffer", async () => {
		const frameTransport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 2,
			payloadCodec: "json",
			stderrText: () => "stderr",
		});
		const fsChanged = (): LiveEventFrame => ({
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: {
				type: "structured",
				name: "filesystem.changed",
				detail: { dirs: '["/tmp"]', overflow: "false" },
			},
		});

		// A recurring change stream on a busy VM far exceeds the buffer
		// capacity; if any event were buffered, the fail-closed overflow would
		// kill the client and the waiter below would reject.
		let seen = 0;
		const dispose = client.onEvent((event) => {
			if (
				event.payload.type === "structured" &&
				event.payload.name === "filesystem.changed"
			) {
				seen += 1;
			}
		});
		for (let i = 0; i < 8; i += 1) {
			frameTransport.emitFrame(fsChanged());
		}

		const waiter = client.waitForEvent(
			(event) => event.payload.type === "vm_lifecycle",
		);
		frameTransport.emitFrame({
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "vm_lifecycle", state: "ready" },
		});

		await expect(waiter).resolves.toMatchObject({
			payload: { type: "vm_lifecycle", state: "ready" },
		});
		expect(seen).toBe(8);
		dispose();
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
		const { stdin, stdout, client } = createClient();
		const written = readWrittenFrame(stdin);
		client.setSidecarRequestHandler(async () => ({
			type: "host_callback_result",
			invocation_id: "invocation",
			result: { ok: true },
		}));

		writeIncomingFrame(stdout, {
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
});
