import type { Readable, Writable } from "node:stream";
import type { ByteArray } from "./framing.js";
import {
	encodeLengthPrefixedPayload,
	tryDecodeLengthPrefixedPayload,
} from "./framing.js";

export interface StdioFrameTransportOptions<TReadFrame, TWriteFrame> {
	stdin: Writable;
	stdout: Readable;
	encodeFrame: (frame: TWriteFrame) => Uint8Array;
	decodeFrame: (payload: Uint8Array) => TReadFrame;
}

export interface FrameTransport<TReadFrame, TWriteFrame = TReadFrame> {
	onFrame(handler: (frame: TReadFrame) => void): () => void;
	onError(handler: (error: Error) => void): () => void;
	onEnd(handler: () => void): () => void;
	writeFrame(frame: TWriteFrame): Promise<void>;
	dispose(): void;
}

export class StdioFrameTransport<TReadFrame, TWriteFrame = TReadFrame>
	implements FrameTransport<TReadFrame, TWriteFrame>
{
	private readonly stdin: Writable;
	private readonly stdout: Readable;
	private readonly encodeFrame: (frame: TWriteFrame) => Uint8Array;
	private readonly decodeFrame: (payload: Uint8Array) => TReadFrame;
	private readonly frameListeners = new Set<(frame: TReadFrame) => void>();
	private readonly errorListeners = new Set<(error: Error) => void>();
	private readonly endListeners = new Set<() => void>();
	private stdoutBuffer: ByteArray = new Uint8Array(0);

	constructor(options: StdioFrameTransportOptions<TReadFrame, TWriteFrame>) {
		this.stdin = options.stdin;
		this.stdout = options.stdout;
		this.encodeFrame = options.encodeFrame;
		this.decodeFrame = options.decodeFrame;
		this.stdout.on("data", this.handleData);
		this.stdout.on("end", this.handleEnd);
		this.stdout.on("error", this.handleError);
	}

	onFrame(handler: (frame: TReadFrame) => void): () => void {
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

	async writeFrame(frame: TWriteFrame): Promise<void> {
		const payload = this.encodeFrame(frame);
		const encoded = encodeLengthPrefixedPayload(payload);
		await new Promise<void>((resolve, reject) => {
			this.stdin.write(encoded, (error) => {
				if (error) {
					reject(error);
					return;
				}
				resolve();
			});
		});
	}

	dispose(): void {
		this.stdout.off("data", this.handleData);
		this.stdout.off("end", this.handleEnd);
		this.stdout.off("error", this.handleError);
		this.frameListeners.clear();
		this.errorListeners.clear();
		this.endListeners.clear();
	}

	private readonly handleData = (chunk: ByteArray | string): void => {
		const bytes: ByteArray =
			typeof chunk === "string"
				? new TextEncoder().encode(chunk)
				: new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
		this.stdoutBuffer = concatBytes(this.stdoutBuffer, bytes);
		this.drainFrames();
	};

	private readonly handleEnd = (): void => {
		for (const listener of this.endListeners) {
			listener();
		}
	};

	private readonly handleError = (error: unknown): void => {
		const normalized = error instanceof Error ? error : new Error(String(error));
		for (const listener of this.errorListeners) {
			listener(normalized);
		}
	};

	private drainFrames(): void {
		for (;;) {
			const decoded = tryDecodeLengthPrefixedPayload(this.stdoutBuffer);
			if (!decoded) {
				return;
			}
			this.stdoutBuffer = decoded.remaining;
			let frame: TReadFrame;
			try {
				frame = this.decodeFrame(decoded.payload);
			} catch (error) {
				this.handleError(error);
				continue;
			}
			for (const listener of this.frameListeners) {
				listener(frame);
			}
		}
	}
}

function concatBytes(left: ByteArray, right: ByteArray): ByteArray {
	if (left.length === 0) {
		return right;
	}
	if (right.length === 0) {
		return left;
	}
	const combined = new Uint8Array(left.length + right.length);
	combined.set(left);
	combined.set(right, left.length);
	return combined;
}
