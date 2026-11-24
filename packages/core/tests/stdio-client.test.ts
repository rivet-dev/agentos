import { spawn } from "node:child_process";
import { EventEmitter, once } from "node:events";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Duplex, PassThrough, Writable } from "node:stream";
import { describe, expect, test } from "vitest";
import { encodeLengthPrefixedPayload } from "../src/framing.js";
import { StdioSidecarProcess } from "../src/process.js";
import type { SidecarProtocolClient } from "../src/protocol-client.js";
import { encodeProtocolFramePayload } from "../src/protocol-frames.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";
import { SidecarProcessExited } from "../src/sidecar-errors.js";
import { SidecarProcess } from "../src/sidecar-process.js";
import { StdioSidecarProtocolClient } from "../src/stdio-client.js";

const ownership = {
	scope: "connection" as const,
	connection_id: "conn",
};

describe("stdio sidecar protocol client", () => {
	test("keeps the control channel open while the peer finishes shutdown", async () => {
		const child = spawn(
			process.execPath,
			[
				"--input-type=module",
				"-e",
				`
import { Socket } from "node:net";
const control = new Socket({ fd: 3, readable: true, writable: true, allowHalfOpen: true });
let buffer = Buffer.alloc(0);
let released = false;
control.on("data", chunk => {
  buffer = Buffer.concat([buffer, chunk]);
  while (buffer.length >= 4) {
    const length = buffer.readUInt32BE(0);
    if (length > 8192) throw new Error("unexpected oversized control frame");
    if (buffer.length < length + 4) return;
    const frame = JSON.parse(buffer.subarray(4, length + 4).toString("utf8"));
    buffer = buffer.subarray(length + 4);
    if (frame.frame_type !== "control" || frame.payload.type !== "shutdown") {
      throw new Error("expected shutdown control frame");
    }
    process.send("shutdown-received");
  }
});
control.on("end", () => {
  if (!released) {
    process.exitCode = 17;
    control.destroy();
    process.disconnect();
  }
});
process.on("message", message => {
  if (message !== "release") throw new Error("unexpected fixture message");
  released = true;
  control.destroy();
  process.disconnect();
});
process.stdin.resume();
process.send("ready");
`,
			],
			{ stdio: ["pipe", "pipe", "pipe", "pipe", "ipc"] },
		);
		const failures: unknown[] = [];
		let client: StdioSidecarProtocolClient | undefined;
		let disposal:
			| Promise<{ ok: true } | { ok: false; error: unknown }>
			| undefined;
		let removeCloseListener: (() => void) | undefined;
		let releaseSent = false;
		const release = async () => {
			if (releaseSent || !child.connected) return;
			releaseSent = true;
			await new Promise<void>((resolve, reject) => {
				child.send("release", (error) => (error ? reject(error) : resolve()));
			});
		};
		try {
			const sidecarProcess = StdioSidecarProcess.fromChild(
				child as unknown as Parameters<typeof StdioSidecarProcess.fromChild>[0],
			);
			client = Reflect.construct(StdioSidecarProtocolClient, [
				sidecarProcess,
				{
					eventBufferCapacity: 8,
					gracefulExitMs: 5_000,
					forceExitMs: 2_000,
					disposedErrorMessage: "disposed",
					payloadCodec: "json",
				},
			]) as StdioSidecarProtocolClient;
			const [ready] = await once(child, "message", {
				signal: AbortSignal.timeout(2_000),
			});
			expect(ready).toBe("ready");
			const shutdownReceived = once(child, "message", {
				signal: AbortSignal.timeout(2_000),
			});
			const closeEvents = new EventEmitter();
			const closeObserved = once(closeEvents, "closed", {
				signal: AbortSignal.timeout(2_000),
			});
			removeCloseListener = client.onClose((error) => {
				closeEvents.emit("closed", error);
			});
			disposal = client.dispose().then(
				() => ({ ok: true as const }),
				(error: unknown) => ({ ok: false as const, error }),
			);
			const [[message], [closeError]] = await Promise.all([
				shutdownReceived,
				closeObserved,
			]);
			expect(message).toBe("shutdown-received");
			expect(closeError).toBeInstanceOf(Error);
			expect(closeError.message).toBe("disposed");
			expect(child.exitCode).toBeNull();
			expect(child.signalCode).toBeNull();
			expect((child.stdio[3] as Duplex).writableEnded).toBe(false);
			await release();
			const result = await disposal;
			if (!result.ok) throw result.error;
			expect(child.exitCode).toBe(0);
			expect(child.signalCode).toBeNull();
		} catch (error) {
			failures.push(error);
		} finally {
			try {
				await release();
			} catch (error) {
				failures.push(error);
			}
			if (client) {
				disposal ??= client.dispose().then(
					() => ({ ok: true as const }),
					(error: unknown) => ({ ok: false as const, error }),
				);
				const result = await disposal;
				if (!result.ok && !failures.includes(result.error)) {
					failures.push(result.error);
				}
			} else if (
				child.pid &&
				child.exitCode === null &&
				child.signalCode === null
			) {
				try {
					const exited = once(child, "exit", {
						signal: AbortSignal.timeout(2_000),
					});
					child.kill("SIGKILL");
					await exited;
				} catch (error) {
					failures.push(error);
				}
			}
			removeCloseListener?.();
		}
		if (failures.length) {
			throw new AggregateError(failures, "stdio shutdown fixture failed");
		}
	});

	test("classifies a broken write before the child exit event", async () => {
		const brokenPipe = Object.assign(new Error("write EPIPE"), {
			code: "EPIPE",
		});
		const stdin = new Writable({
			write(_chunk, _encoding, callback) {
				callback(brokenPipe);
			},
		});
		const stdout = new PassThrough();
		const stderr = new PassThrough();
		const control = new PassThrough();
		const child = Object.assign(new EventEmitter(), {
			stdin,
			stdout,
			stderr,
			stdio: [stdin, stdout, stderr, control],
			exitCode: null,
			signalCode: null as string | null,
		});
		const process = StdioSidecarProcess.fromChild(
			child as unknown as Parameters<typeof StdioSidecarProcess.fromChild>[0],
		);
		const client = Reflect.construct(StdioSidecarProtocolClient, [
			process,
			{
				eventBufferCapacity: 8,
				gracefulExitMs: 0,
				forceExitMs: 0,
				disposedErrorMessage: "disposed",
				payloadCodec: "json",
			},
		]) as StdioSidecarProtocolClient;
		let closed: Error | undefined;
		client.onClose((error) => {
			closed = error;
		});

		const error = await client
			.sendRequest({
				ownership,
				payload: { type: "create_layer" },
			})
			.catch((caught: unknown) => caught);
		expect(error).toBeInstanceOf(SidecarProcessExited);
		expect(error).toMatchObject({
			exitCode: null,
			signal: null,
			cause: brokenPipe,
		});
		expect(closed).toBeInstanceOf(SidecarProcessExited);
		await expect(
			client.sendRequest({ ownership, payload: { type: "create_layer" } }),
		).rejects.toBeInstanceOf(SidecarProcessExited);
		child.signalCode = "SIGKILL";
		child.emit("exit", null, "SIGKILL");
		await expect(
			client.sendRequest({ ownership, payload: { type: "create_layer" } }),
		).rejects.toMatchObject({ signal: "SIGKILL" });
		client.failPermanently(new Error("disposed"));
	});

	test("classifies a broken callback response before the child exit event", async () => {
		const { client, control, brokenPipe } = createBrokenControlClient();
		const closed = new Promise<Error>((resolve) => client.onClose(resolve));
		client.setSidecarRequestHandler(() => ({
			type: "host_callback_result",
			invocation_id: "invocation",
			result: { ok: true },
		}));
		control.push(
			encodeLengthPrefixedPayload(
				encodeProtocolFramePayload(
					{
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
					},
					"json",
				),
			),
		);

		await expect(closed).resolves.toMatchObject({
			name: "SidecarProcessExited",
			cause: brokenPipe,
		});
		await expect(
			client.sendRequest({ ownership, payload: { type: "create_layer" } }),
		).rejects.toBeInstanceOf(SidecarProcessExited);
	});

	test("classifies a broken shutdown control write before the child exit event", async () => {
		const { client, brokenPipe } = createBrokenControlClient();
		let closed: Error | undefined;
		client.onClose((error) => {
			closed = error;
		});
		const protocol = Reflect.get(
			client,
			"protocolClient",
		) as SidecarProtocolClient;
		await expect(protocol.shutdown("test complete")).rejects.toMatchObject({
			name: "SidecarProcessExited",
			cause: brokenPipe,
		});
		expect(closed).toBeInstanceOf(SidecarProcessExited);
	});
	test("drives a stdio protocol process", async () => {
		const fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-client-"));
		const driverPath = join(fixtureRoot, "fake-sidecar.mjs");
		writeFileSync(
			driverPath,
			[
				"import { Socket } from 'node:net';",
				"const control = new Socket({ fd: 3, readable: true, writable: true });",
				"let stdinBuffer = Buffer.alloc(0);",
				"const writeFrame = (stream, frame) => {",
				"  const payload = Buffer.from(JSON.stringify(frame));",
				"  const header = Buffer.alloc(4);",
				"  header.writeUInt32BE(payload.length, 0);",
				"  stream.write(Buffer.concat([header, payload]));",
				"};",
				"const handleFrame = (frame) => {",
				"  if (frame.payload.type !== 'create_layer') {",
				"    throw new Error('unexpected payload ' + frame.payload.type);",
				"  }",
				"  writeFrame(control, {",
				"    frame_type: 'response',",
				"    schema: frame.schema,",
				"    request_id: frame.request_id,",
				"    ownership: frame.ownership,",
				"    payload: { type: 'layer_created', layer_id: 'layer-1' },",
				"  });",
				"  writeFrame(process.stdout, {",
				"    frame_type: 'event',",
				"    schema: frame.schema,",
				"    ownership: frame.ownership,",
				"    payload: { type: 'structured', name: 'ready', detail: {} },",
				"  });",
				"};",
				"const drain = () => {",
				"  while (stdinBuffer.length >= 4) {",
				"    const length = stdinBuffer.readUInt32BE(0);",
				"    if (stdinBuffer.length < 4 + length) return;",
				"    const payload = stdinBuffer.subarray(4, 4 + length);",
				"    stdinBuffer = stdinBuffer.subarray(4 + length);",
				"    handleFrame(JSON.parse(payload.toString('utf8')));",
				"  }",
				"};",
				"process.stdin.on('data', (chunk) => {",
				"  stdinBuffer = Buffer.concat([stdinBuffer, Buffer.from(chunk)]);",
				"  drain();",
				"});",
				"process.stdin.resume();",
			].join("\n"),
		);

		const client = StdioSidecarProtocolClient.spawn({
			command: process.execPath,
			args: [driverPath],
			eventBufferCapacity: 8,
			payloadCodec: "json",
		});

		try {
			const ready = client.waitForEvent({
				type: "structured",
				name: "ready",
			});
			await expect(
				client.sendRequest({
					ownership,
					payload: { type: "create_layer" },
				}),
			).resolves.toMatchObject({
				payload: { type: "layer_created", layer_id: "layer-1" },
			});
			await expect(ready).resolves.toMatchObject({
				payload: { type: "structured", name: "ready" },
			});
		} finally {
			await client.dispose();
		}
	});

	test("registers native spawn for the shared SidecarProcess wrapper", async () => {
		const fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-process-"));
		const driverPath = join(fixtureRoot, "fake-sidecar.mjs");
		writeFileSync(
			driverPath,
			[
				"import { Socket } from 'node:net';",
				"const control = new Socket({ fd: 3, readable: true, writable: true });",
				"let stdinBuffer = Buffer.alloc(0);",
				"const writeFrame = (stream, frame) => {",
				"  const payload = Buffer.from(JSON.stringify(frame));",
				"  const header = Buffer.alloc(4);",
				"  header.writeUInt32BE(payload.length, 0);",
				"  stream.write(Buffer.concat([header, payload]));",
				"};",
				"const handleFrame = (frame) => {",
				"  writeFrame(control, {",
				"    frame_type: 'response',",
				"    schema: frame.schema,",
				"    request_id: frame.request_id,",
				"    ownership: frame.ownership,",
				"    payload: { type: 'layer_created', layer_id: 'layer-1' },",
				"  });",
				"};",
				"const drain = () => {",
				"  while (stdinBuffer.length >= 4) {",
				"    const length = stdinBuffer.readUInt32BE(0);",
				"    if (stdinBuffer.length < 4 + length) return;",
				"    const payload = stdinBuffer.subarray(4, 4 + length);",
				"    stdinBuffer = stdinBuffer.subarray(4 + length);",
				"    handleFrame(JSON.parse(payload.toString('utf8')));",
				"  }",
				"};",
				"process.stdin.on('data', (chunk) => {",
				"  stdinBuffer = Buffer.concat([stdinBuffer, Buffer.from(chunk)]);",
				"  drain();",
				"});",
				"process.stdin.resume();",
			].join("\n"),
		);

		const sidecarProcess = SidecarProcess.spawn({
			command: process.execPath,
			args: [driverPath],
			eventBufferCapacity: 8,
			payloadCodec: "json",
		});

		try {
			await expect(
				sidecarProcess.createLayer(
					{ connectionId: "conn", sessionId: "session" },
					{ vmId: "vm" },
				),
			).resolves.toBe("layer-1");
		} finally {
			await sidecarProcess.dispose();
		}
	});
});

function createBrokenControlClient(): {
	client: StdioSidecarProtocolClient;
	control: Duplex;
	brokenPipe: Error;
} {
	const brokenPipe = Object.assign(new Error("write EPIPE"), { code: "EPIPE" });
	const control = new Duplex({
		read() {},
		write(_chunk, _encoding, callback) {
			callback(brokenPipe);
		},
	});
	const stdin = new PassThrough();
	const stdout = new PassThrough();
	const stderr = new PassThrough();
	const child = Object.assign(new EventEmitter(), {
		stdin,
		stdout,
		stderr,
		stdio: [stdin, stdout, stderr, control],
		exitCode: null,
		signalCode: null,
	});
	const process = StdioSidecarProcess.fromChild(
		child as unknown as Parameters<typeof StdioSidecarProcess.fromChild>[0],
	);
	const client = Reflect.construct(StdioSidecarProtocolClient, [
		process,
		{
			eventBufferCapacity: 8,
			gracefulExitMs: 0,
			forceExitMs: 0,
			disposedErrorMessage: "disposed",
			payloadCodec: "json",
		},
	]) as StdioSidecarProtocolClient;
	return { client, control, brokenPipe };
}
