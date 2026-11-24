import { PassThrough, Writable } from "node:stream";
import { describe, expect, test, vi } from "vitest";
import { StdioFrameTransport } from "../src/frame-stream.js";
import { encodeLengthPrefixedPayload } from "../src/framing.js";

function concatBytes(...chunks: Uint8Array[]): Uint8Array {
	const totalLength = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
	const combined = new Uint8Array(totalLength);
	let offset = 0;
	for (const chunk of chunks) {
		combined.set(chunk, offset);
		offset += chunk.length;
	}
	return combined;
}

describe("stdio frame transport", () => {
	test("decodes complete frames from partial stdout chunks", async () => {
		const stdin = new PassThrough();
		const stdout = new PassThrough();
		const transport = new StdioFrameTransport<string>({
			stdin,
			stdout,
			encodeFrame: (frame) => Buffer.from(frame, "utf8"),
			decodeFrame: (payload) => Buffer.from(payload).toString("utf8"),
		});
		const frames = new Promise<string[]>((resolve) => {
			const seen: string[] = [];
			transport.onFrame((frame) => {
				seen.push(frame);
				if (seen.length === 2) {
					resolve(seen);
				}
			});
		});
		const first = encodeLengthPrefixedPayload(Buffer.from("one"));
		const second = encodeLengthPrefixedPayload(Buffer.from("two"));
		const combined = concatBytes(first, second);

		stdout.write(combined.subarray(0, 5));
		stdout.write(combined.subarray(5));

		await expect(frames).resolves.toEqual(["one", "two"]);
		transport.dispose();
	});

	test("writes length-prefixed frames to stdin", async () => {
		const stdin = new PassThrough();
		const stdout = new PassThrough();
		const transport = new StdioFrameTransport<string>({
			stdin,
			stdout,
			encodeFrame: (frame) => Buffer.from(frame, "utf8"),
			decodeFrame: (payload) => Buffer.from(payload).toString("utf8"),
		});
		const written = new Promise<Buffer>((resolve) => {
			stdin.once("data", (chunk: Buffer) => resolve(Buffer.from(chunk)));
		});

		await transport.writeFrame("hello");

		expect([...(await written)]).toEqual([
			...encodeLengthPrefixedPayload(Buffer.from("hello")),
		]);
		transport.dispose();
	});

	test("reports stdin errors instead of emitting an unhandled stream error", async () => {
		const stdin = new PassThrough();
		const stdout = new PassThrough();
		const transport = new StdioFrameTransport<string>({
			stdin,
			stdout,
			encodeFrame: (frame) => Buffer.from(frame, "utf8"),
			decodeFrame: (payload) => Buffer.from(payload).toString("utf8"),
		});
		const error = new Promise<Error>((resolve) => {
			transport.onError(resolve);
		});

		const brokenPipe = Object.assign(new Error("write EPIPE"), {
			code: "EPIPE",
		});
		stdin.destroy(brokenPipe);

		await expect(error).resolves.toBe(brokenPipe);
		await expect(transport.writeFrame("late")).rejects.toThrow();
		transport.dispose();
	});

	test("uses one error listener when stdin and stdout share a duplex stream", () => {
		const stdio = new PassThrough();
		const initialErrorListeners = stdio.listenerCount("error");
		const transport = new StdioFrameTransport<string>({
			stdin: stdio,
			stdout: stdio,
			encodeFrame: (frame) => Buffer.from(frame, "utf8"),
			decodeFrame: (payload) => Buffer.from(payload).toString("utf8"),
		});
		const errors: Error[] = [];
		transport.onError((error) => errors.push(error));

		expect(stdio.listenerCount("error")).toBe(initialErrorListeners + 1);
		const brokenPipe = Object.assign(new Error("write EPIPE"), {
			code: "EPIPE",
		});
		stdio.emit("error", brokenPipe);
		expect(errors).toEqual([brokenPipe]);

		transport.dispose();
		expect(stdio.listenerCount("error")).toBe(initialErrorListeners);
	});

	test("a write that fails after dispose is logged instead of crashing the host", async () => {
		let finishWrite: ((error?: Error | null) => void) | undefined;
		const stdin = new Writable({
			write(_chunk, _encoding, callback) {
				finishWrite = callback;
			},
		});
		const transport = new StdioFrameTransport<string>({
			stdin,
			stdout: new PassThrough(),
			encodeFrame: (frame) => Buffer.from(frame, "utf8"),
			decodeFrame: (payload) => Buffer.from(payload).toString("utf8"),
		});
		const uncaught: unknown[] = [];
		const recordUncaught = (error: unknown) => uncaught.push(error);
		process.on("uncaughtException", recordUncaught);
		const logged = vi.spyOn(console, "error").mockImplementation(() => {});
		try {
			const write = transport.writeFrame("in flight");
			transport.dispose();
			const brokenPipe = Object.assign(new Error("write EPIPE"), {
				code: "EPIPE",
			});
			finishWrite?.(brokenPipe);

			await expect(write).rejects.toBe(brokenPipe);
			await new Promise((resolve) => setImmediate(resolve));
			expect(uncaught).toEqual([]);
			expect(logged).toHaveBeenCalledWith(expect.any(String), brokenPipe);
		} finally {
			logged.mockRestore();
			process.off("uncaughtException", recordUncaught);
		}
	});

	test("reports decode errors", async () => {
		const stdin = new PassThrough();
		const stdout = new PassThrough();
		const transport = new StdioFrameTransport<string>({
			stdin,
			stdout,
			encodeFrame: (frame) => Buffer.from(frame, "utf8"),
			decodeFrame: () => {
				throw new Error("bad frame");
			},
		});
		const error = new Promise<Error>((resolve) => {
			transport.onError(resolve);
		});

		stdout.write(encodeLengthPrefixedPayload(Buffer.from("bad")));

		expect((await error).message).toBe("bad frame");
		transport.dispose();
	});
});
