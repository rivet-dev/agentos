import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
	SidecarProcessExited,
	SidecarRejectedError,
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
	nextWriteError: Error | null = null;
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
		if (this.nextWriteError) {
			const error = this.nextWriteError;
			this.nextWriteError = null;
			throw error;
		}
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
	it("classifies callback write failure before a transport error event", async () => {
		const frameTransport = new MemoryFrameTransport();
		const brokenPipe = Object.assign(new Error("write EPIPE"), {
			code: "EPIPE",
		});
		frameTransport.nextWriteError = brokenPipe;
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 2,
			payloadCodec: "json",
			fatalWriteError: (error) =>
				error === brokenPipe
					? new SidecarProcessExited({
							exitCode: null,
							signal: null,
							stderr: "",
							cause: error,
						})
					: undefined,
		});
		client.setSidecarRequestHandler(() => ({
			type: "host_callback_result",
			invocation_id: "invocation",
			result: { ok: true },
		}));
		const closed = new Promise<Error>((resolve) => client.onClose(resolve));
		frameTransport.emitFrame({
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
		await expect(closed).resolves.toMatchObject({
			name: "SidecarProcessExited",
			cause: brokenPipe,
		});
		client.dispose();
	});
	it("keeps a local write failure nonterminal even when frame errors are wrapped", async () => {
		const frameTransport = new MemoryFrameTransport();
		const localError = new Error("local write failed");
		frameTransport.nextWriteError = localError;
		const client = new SidecarProtocolClient({
			frameTransport,
			eventBufferCapacity: 2,
			payloadCodec: "json",
			frameError: (error) => new Error(`wrapped: ${error.message}`),
		});
		const closed: Error[] = [];
		client.onClose((error) => closed.push(error));

		await expect(
			client.sendRequest({ ownership, payload: { type: "create_layer" } }),
		).rejects.toBe(localError);
		expect(closed).toEqual([]);

		const nextRequest = client.sendRequest({
			ownership,
			payload: { type: "create_layer" },
		});
		await expect.poll(() => frameTransport.writes.length).toBe(1);
		frameTransport.emitFrame({
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 2,
			ownership,
			payload: { type: "layer_created", layer_id: "after-local-error" },
		});
		await expect(nextRequest).resolves.toMatchObject({
			payload: { layer_id: "after-local-error" },
		});
		client.dispose();
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

	it("reports observer failures and delivers events without waiting for observers", async () => {
		const { stdout, client } = createClient();
		const reported = vi
			.spyOn(console, "error")
			.mockImplementation(() => undefined);
		const syncError = new Error("sync observer");
		const asyncError = new Error("async observer");
		let rejectObserver!: (reason: Error) => void;
		const completion = new Promise<never>((_resolve, reject) => {
			rejectObserver = reject;
		});
		const sibling = vi.fn();
		try {
			client.onEvent(() => {
				throw syncError;
			});
			client.onEvent(() => completion);
			client.onEvent(sibling);
			const event = client.waitForEvent(
				{ type: "structured", name: "ready" },
				1_000,
			);
			const frame: LiveEventFrame = {
				frame_type: "event",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				ownership,
				payload: { type: "structured", name: "ready", detail: {} },
			};
			writeIncomingFrame(stdout, frame);
			expect(sibling.mock.calls).toEqual([[frame]]);
			await expect(event).resolves.toEqual(frame);

			// Keep a failing baseline from leaking a test-owned rejection.
			const rejected = expect(completion).rejects.toBe(asyncError);
			rejectObserver(asyncError);
			await rejected;
			const diagnostic =
				"ERR_AGENTOS_SIDECAR_EVENT_OBSERVER: event observer failed";
			expect(reported.mock.calls).toEqual([
				[diagnostic, syncError],
				[diagnostic, asyncError],
			]);
		} finally {
			client.dispose();
			reported.mockRestore();
		}
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
	it("watchdog does not count a stalled host as a full silence window", async () => {
		let now = 0;
		let tick: (() => void) | undefined;
		vi.spyOn(performance, "now").mockImplementation(() => now);
		vi.spyOn(globalThis, "setInterval").mockImplementation((callback) => {
			tick = callback as () => void;
			return { unref() {} } as ReturnType<typeof setInterval>;
		});
		vi.spyOn(globalThis, "clearInterval").mockImplementation(() => {});
		let expired = 0;
		const transport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport: transport,
			payloadCodec: "json",
			eventBufferCapacity: 8,
			silenceTimeoutMs: 100,
			onSilenceExpired: () => {
				expired += 1;
			},
		});
		try {
			now = 1000;
			tick?.();
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(expired).toBe(0);
		} finally {
			client.dispose();
			vi.restoreAllMocks();
		}
	});

	it("watchdog resets its anchor and cancels stale confirmation", async () => {
		let now = 0;
		let tick: (() => void) | undefined;
		vi.spyOn(performance, "now").mockImplementation(() => now);
		vi.spyOn(globalThis, "setInterval").mockImplementation((callback) => {
			tick = callback as () => void;
			return { unref() {} } as ReturnType<typeof setInterval>;
		});
		vi.spyOn(globalThis, "clearInterval").mockImplementation(() => {});
		let expired = 0;
		const transport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport: transport,
			payloadCodec: "json",
			eventBufferCapacity: 8,
			silenceTimeoutMs: 100,
			onSilenceExpired: () => {
				expired += 1;
			},
		});
		const beat = () =>
			transport.emitFrame({
				frame_type: "event",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				ownership,
				payload: { type: "structured", name: "heartbeat", detail: {} },
			});
		try {
			for (const time of [25, 50, 75]) {
				now = time;
				tick?.();
			}
			now = 99;
			beat();
			for (const time of [100, 125, 150, 175]) {
				now = time;
				tick?.();
			}
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(expired).toBe(0);
			now = 200;
			tick?.();
			// Real inbound activity after threshold but before confirmation wins.
			now = 201;
			beat();
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(expired).toBe(0);
			// Repeated host stalls cannot indefinitely postpone genuine silence.
			for (const time of [1000, 2000, 3000, 4000, 5000]) {
				now = time;
				tick?.();
			}
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(expired).toBe(1);
			await expect(
				client.sendRequest({ ownership, payload: { type: "create_layer" } }),
			).rejects.toThrow(/sidecar unresponsive/);
		} finally {
			client.dispose();
			vi.restoreAllMocks();
		}
	});

	it("timed event confirmation is cancellable by event abort and close", async () => {
		const timers: Array<() => void> = [];
		vi.spyOn(globalThis, "setTimeout").mockImplementation((callback) => {
			timers.push(callback as () => void);
			return {} as ReturnType<typeof setTimeout>;
		});
		vi.spyOn(globalThis, "clearTimeout").mockImplementation(() => {});
		const transport = new MemoryFrameTransport();
		const client = new SidecarProtocolClient({
			frameTransport: transport,
			payloadCodec: "json",
			eventBufferCapacity: 8,
		});
		const event: LiveEventFrame = {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "structured", name: "probe", detail: {} },
		};
		try {
			const first = client.waitForEvent(() => true, 10);
			timers.shift()?.();
			transport.emitFrame(event);
			await expect(first).resolves.toEqual(event);
			const abort = new AbortController();
			const second = client.waitForEvent(() => true, 10, {
				signal: abort.signal,
			});
			const rejected = expect(second).rejects.toThrow(/abort/i);
			timers.shift()?.();
			abort.abort();
			await rejected;
			const third = client.waitForEvent(() => true, 10);
			const closed = expect(third).rejects.toThrow("synthetic close");
			timers.shift()?.();
			client.failPermanently(new Error("synthetic close"));
			await closed;
			await new Promise<void>((resolve) => setImmediate(resolve));
		} finally {
			client.dispose();
			vi.restoreAllMocks();
		}
	});

	it("watchdog gives a buffered real pipe heartbeat one bounded I/O turn", async () => {
		let now = 0;
		let tick: (() => void) | undefined;
		const directory = mkdtempSync(join(tmpdir(), "agentos-watchdog-"));
		const marker = join(directory, "written");
		const frame: LiveEventFrame = {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: { type: "structured", name: "heartbeat", detail: {} },
		};
		const bytes = Buffer.from(
			encodeLengthPrefixedPayload(encodeProtocolFramePayload(frame, "json")),
		).toString("base64");
		// Only synthetic bytes and an owned marker path cross to this real Node
		// child. Its stdout write callback proves the frame was put in the pipe.
		const child = spawn(
			process.execPath,
			[
				"--input-type=module",
				"-e",
				`
import { writeFileSync } from 'node:fs';
process.stdin.once('data', () => process.stdout.write(Buffer.from(process.argv[2], 'base64'), () => writeFileSync(process.argv[1], 'written')));
process.stderr.write('READY');
`,
				marker,
				bytes,
			],
			{ stdio: ["pipe", "pipe", "pipe"] },
		);
		let childError: Error | undefined;
		child.once("error", (error) => {
			childError = error;
		});
		const closed = new Promise<void>((resolve) => {
			child.once("close", () => resolve());
		});
		let ready = "";
		child.stderr.on("data", (chunk: Buffer) => {
			ready += chunk.toString();
		});
		let client: SidecarProtocolClient | undefined;
		try {
			await expect
				.poll(
					() => {
						if (childError) throw childError;
						return ready;
					},
					{ timeout: 1000, interval: 5 },
				)
				.toBe("READY");
			vi.spyOn(performance, "now").mockImplementation(() => now);
			vi.spyOn(globalThis, "setInterval").mockImplementation((callback) => {
				tick = callback as () => void;
				return { unref() {} } as ReturnType<typeof setInterval>;
			});
			vi.spyOn(globalThis, "clearInterval").mockImplementation(() => {});
			let expired = 0;
			client = new SidecarProtocolClient({
				stdin: child.stdin,
				stdout: child.stdout,
				combinedStdio: true,
				payloadCodec: "json",
				eventBufferCapacity: 8,
				silenceTimeoutMs: 100,
				onSilenceExpired: () => {
					expired += 1;
				},
			});
			for (const time of [25, 50, 75]) {
				now = time;
				tick?.();
			}
			child.stdin.write("beat");
			// Finite blocking sleeps deliberately hold this host's reader while
			// the child writes. No CPU/heap/FD saturation and no external network.
			for (let attempt = 0; attempt < 50 && !existsSync(marker); attempt += 1) {
				Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 20);
			}
			expect(existsSync(marker)).toBe(true);
			now = 1000;
			tick?.(); // capped last interval reaches the threshold.
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(expired).toBe(0);
			// This heartbeat must actually reset the counter, not just be delayed
			// behind an unchecked second grace. Three ticks cannot expire it.
			for (const time of [1025, 1050, 1075]) {
				now = time;
				tick?.();
			}
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(expired).toBe(0);
		} finally {
			client?.dispose();
			vi.restoreAllMocks();
			if (child.exitCode === null && child.signalCode === null)
				child.kill("SIGKILL");
			let timer: ReturnType<typeof setTimeout> | undefined;
			try {
				await Promise.race([
					closed,
					new Promise<never>((_, reject) => {
						timer = setTimeout(
							() => reject(new Error("watchdog child was not reaped")),
							1000,
						);
					}),
				]);
			} finally {
				if (timer) clearTimeout(timer);
				rmSync(directory, { recursive: true, force: true });
			}
		}
	}, 4000);

	it("does not arm a watchdog after synchronous terminal subscription replay", async () => {
		for (const replay of ["error", "end"] as const) {
			class ClosedTransport extends MemoryFrameTransport {
				override onError(handler: (error: Error) => void): () => void {
					if (replay === "error")
						handler(new Error("synthetic replayed error"));
					return super.onError(handler);
				}
				override onEnd(handler: () => void): () => void {
					if (replay === "end") handler();
					return super.onEnd(handler);
				}
			}
			const interval = vi.spyOn(globalThis, "setInterval");
			const client = new SidecarProtocolClient({
				frameTransport: new ClosedTransport(),
				eventBufferCapacity: 8,
				payloadCodec: "json",
			});
			try {
				expect(interval).not.toHaveBeenCalled();
				await expect(
					client.sendRequest({ ownership, payload: { type: "create_layer" } }),
				).rejects.toThrow(
					replay === "error"
						? "synthetic replayed error"
						: "sidecar protocol stream ended",
				);
			} finally {
				client.dispose();
				interval.mockRestore();
			}
		}
	});
});

function ingressRefusal(): Extract<LiveResponseFrame['payload'],{type:'rejected'}> {
 return {type:'rejected',code:'ERR_AGENTOS_PROTOCOL_FRAME_LIMIT',message:'ordinary ingress full',limit_name:'stdio ordinary ingress',configured_limit:128,current_usage:128,requested:1,unit:'frames',scope:'process',vm_id:null,session_generation:null,capability_id:null,operation:'stdio.requestAdmission',configuration_path:'runtime.protocol.maxIngressFrames',retryable:true,errno:'EAGAIN'};
}
it('retries only deterministic ingress refusals with the same immutable request',async()=>{
 const transport=new MemoryFrameTransport(),client=new SidecarProtocolClient({frameTransport:transport,eventBufferCapacity:2,payloadCodec:'json'}),scope={...ownership};
 try{
  const response=client.sendRequest({ownership:scope,payload:{type:'create_layer'}}).then(value=>({ok:true as const,value}),error=>({ok:false as const,error}));
  await expect.poll(()=>transport.writes.length).toBe(1);scope.connection_id='mutated';
  transport.emitFrame({frame_type:'response',schema:SIDECAR_PROTOCOL_SCHEMA,request_id:1,ownership,payload:ingressRefusal()});
  await expect.poll(()=>transport.writes.length,{timeout:1000}).toBe(2);
  expect(transport.writes[1]).toEqual(transport.writes[0]);expect(transport.writes[1]).toMatchObject({request_id:1,ownership:{connection_id:'conn'}});
  transport.emitFrame({frame_type:'response',schema:SIDECAR_PROTOCOL_SCHEMA,request_id:1,ownership,payload:{type:'layer_created',layer_id:'once'}});
  expect(await response).toMatchObject({ok:true,value:{payload:{layer_id:'once'}}});
 }finally{client.dispose();}
});
it('closing the client cancels an ingress retry before another write',async()=>{
 const transport=new MemoryFrameTransport(),client=new SidecarProtocolClient({frameTransport:transport,eventBufferCapacity:2,payloadCodec:'json'});
 const response=client.sendRequest({ownership,payload:{type:'create_layer'}}).catch(error=>error);
 await expect.poll(()=>transport.writes.length).toBe(1);
 transport.emitFrame({frame_type:'response',schema:SIDECAR_PROTOCOL_SCHEMA,request_id:1,ownership,payload:ingressRefusal()});
 await Promise.resolve();client.dispose();expect(await response).toBeInstanceOf(Error);await new Promise(resolve=>setTimeout(resolve,20));expect(transport.writes).toHaveLength(1);
});
