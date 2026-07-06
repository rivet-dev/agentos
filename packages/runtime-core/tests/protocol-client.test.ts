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
		frameTimeoutMs: 1_000,
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
			frameTimeoutMs: 1_000,
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
