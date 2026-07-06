import type { Readable, Writable } from "node:stream";
import {
	SidecarEventBuffer,
	SidecarEventBufferOverflow,
	normalizeSidecarEventMatcher,
	sidecarEventWaitAbortError,
	type LiveSidecarEventSelector,
} from "./event-buffer.js";
import { FrameRpcTransport } from "./frame-rpc.js";
import type { FrameTransport } from "./frame-stream.js";
import {
	HostProtocolFrameFactory,
	classifySidecarWrittenProtocolFrame,
	decodeProtocolFramePayload,
	encodeProtocolFramePayload,
	resolveSidecarRequestFramePayload,
	type LiveEventFrame,
	type LiveProtocolFrame,
	type LiveRequestFrame,
	type LiveResponseFrame,
	type LiveSidecarRequestFrame,
	type LiveSidecarRequestHandler,
	type ProtocolFramePayloadCodec,
} from "./protocol-frames.js";
import type { LiveOwnershipScope } from "./ownership.js";
import type { LiveRequestPayload } from "./request-payloads.js";

export interface SidecarProtocolClientOptions {
	frameTransport?: FrameTransport<
		LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
		LiveProtocolFrame
	>;
	stdin?: Writable;
	stdout?: Readable;
	frameTimeoutMs: number;
	eventBufferCapacity: number;
	payloadCodec?: ProtocolFramePayloadCodec;
	stderrText?: () => string;
	frameError?: (error: Error) => Error;
	streamEndedError?: () => Error;
}

export class SidecarProtocolClient {
	private readonly eventBuffer: SidecarEventBuffer<LiveEventFrame>;
	private readonly eventListeners = new Set<(event: LiveEventFrame) => void>();
	private readonly frameTimeoutMs: number;
	private readonly payloadCodec: ProtocolFramePayloadCodec;
	private readonly stderrText: () => string;
	private readonly hostFrameFactory = new HostProtocolFrameFactory();
	private readonly frameTransport: FrameRpcTransport<
		LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
		LiveProtocolFrame,
		LiveResponseFrame,
		LiveEventFrame,
		LiveSidecarRequestFrame
	>;
	private closedError: Error | null = null;
	private readonly eventWaiters = new Set<{
		matches: (event: LiveEventFrame) => boolean;
		resolve: (event: LiveEventFrame) => void;
		reject: (error: Error) => void;
		timer: ReturnType<typeof setTimeout> | null;
	}>();
	private sidecarRequestHandler: LiveSidecarRequestHandler | null = null;

	constructor(options: SidecarProtocolClientOptions) {
		this.frameTimeoutMs = options.frameTimeoutMs;
		this.eventBuffer = new SidecarEventBuffer(options.eventBufferCapacity);
		this.payloadCodec = options.payloadCodec ?? "bare";
		this.stderrText = options.stderrText ?? (() => "");
		this.frameTransport = new FrameRpcTransport<
			LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
			LiveProtocolFrame,
			LiveResponseFrame,
			LiveEventFrame,
			LiveSidecarRequestFrame
		>({
			frameTransport: options.frameTransport,
			stdin: options.stdin,
			stdout: options.stdout,
			encodeFrame: (frame) =>
				encodeProtocolFramePayload(frame, this.payloadCodec),
			decodeFrame: (payload) =>
				decodeProtocolFramePayload(payload, this.payloadCodec),
			classifyFrame: classifySidecarWrittenProtocolFrame,
		});
		this.frameTransport.onEvent((event) => {
			this.dispatchEvent(event);
		});
		this.frameTransport.onSidecarRequest((request) => {
			void this.dispatchSidecarRequest(request);
		});
		this.frameTransport.onError((error) => {
			this.failPermanently(options.frameError?.(error) ?? error);
		});
		this.frameTransport.onEnd(() => {
			this.failPermanently(
				options.streamEndedError?.() ??
					new Error("sidecar protocol stream ended"),
			);
		});
	}

	setSidecarRequestHandler(handler: LiveSidecarRequestHandler | null): void {
		this.sidecarRequestHandler = handler;
	}

	onEvent(handler: (event: LiveEventFrame) => void): () => void {
		this.eventListeners.add(handler);
		return () => {
			this.eventListeners.delete(handler);
		};
	}

	async sendRequest(input: {
		ownership: LiveOwnershipScope;
		payload: LiveRequestPayload;
	}): Promise<LiveResponseFrame> {
		if (this.closedError) {
			throw this.closedError;
		}

		const request = this.hostFrameFactory.createRequestFrame(input);
		const response = await this.frameTransport.sendFrame(
			request.request_id,
			request,
			{
				timeoutMs: this.frameTimeoutMs,
				timeoutMessage: () =>
					`timed out waiting for sidecar protocol frame for ${input.payload.type}\nstderr:\n${this.stderrText()}`,
			},
		);

		if (response.payload.type === "rejected") {
			throw new Error(
				`sidecar rejected request ${request.request_id}: ${response.payload.code}: ${response.payload.message}`,
			);
		}
		return response;
	}

	async waitForEvent(
		matcher:
			| LiveSidecarEventSelector
			| ((event: LiveEventFrame) => boolean),
		timeoutMs?: number,
		options?: {
			signal?: AbortSignal;
		},
	): Promise<LiveEventFrame> {
		if (this.closedError instanceof SidecarEventBufferOverflow) {
			throw this.closedError;
		}
		const normalizedMatcher =
			normalizeSidecarEventMatcher<LiveEventFrame>(matcher);
		const bufferedEvent = this.eventBuffer.take(normalizedMatcher);
		if (bufferedEvent) {
			return bufferedEvent;
		}
		if (this.closedError) {
			throw this.closedError;
		}
		if (options?.signal?.aborted) {
			throw sidecarEventWaitAbortError(options.signal.reason);
		}

		return await new Promise<LiveEventFrame>((resolve, reject) => {
			let abortListener: (() => void) | null = null;
			const waiter = {
				matches: normalizedMatcher.matches,
				resolve: (event: LiveEventFrame) => {
					if (waiter.timer !== null) {
						clearTimeout(waiter.timer);
					}
					if (abortListener) {
						options?.signal?.removeEventListener("abort", abortListener);
						abortListener = null;
					}
					this.eventWaiters.delete(waiter);
					resolve(event);
				},
				reject: (error: Error) => {
					if (waiter.timer !== null) {
						clearTimeout(waiter.timer);
					}
					if (abortListener) {
						options?.signal?.removeEventListener("abort", abortListener);
						abortListener = null;
					}
					this.eventWaiters.delete(waiter);
					reject(error);
				},
				timer:
					timeoutMs === undefined
						? null
						: setTimeout(() => {
								this.eventWaiters.delete(waiter);
								reject(
									new Error(
										`timed out waiting for sidecar event\nstderr:\n${this.stderrText()}`,
									),
								);
							}, timeoutMs),
			};
			if (options?.signal) {
				abortListener = () => {
					waiter.reject(sidecarEventWaitAbortError(options.signal?.reason));
				};
				options.signal.addEventListener("abort", abortListener, { once: true });
			}
			this.eventWaiters.add(waiter);
		});
	}

	failPermanently(
		error: Error,
		options?: {
			replaceExisting?: (current: Error, next: Error) => boolean;
		},
	): void {
		if (this.closedError) {
			if (!options?.replaceExisting?.(this.closedError, error)) {
				return;
			}
		}
		this.closedError = error;
		this.rejectPending(error);
	}

	dispose(): void {
		this.frameTransport.dispose();
	}

	private async writeFrame(frame: LiveProtocolFrame): Promise<void> {
		await this.frameTransport.writeFrame(frame);
	}

	private async dispatchSidecarRequest(
		request: LiveSidecarRequestFrame,
	): Promise<void> {
		const payload = await resolveSidecarRequestFramePayload(
			request,
			this.sidecarRequestHandler,
		);

		try {
			await this.writeFrame(
				this.hostFrameFactory.createSidecarResponseFrame({
					request,
					payload,
				}),
			);
		} catch (error) {
			const normalized =
				error instanceof Error ? error : new Error(String(error));
			this.failPermanently(normalized);
		}
	}

	private dispatchEvent(event: LiveEventFrame): void {
		for (const listener of this.eventListeners) {
			try {
				listener(event);
			} catch {
				// Event listeners are best-effort observers and must not break framing.
			}
		}
		for (const waiter of this.eventWaiters) {
			if (!waiter.matches(event)) {
				continue;
			}
			waiter.resolve(event);
			return;
		}
		this.bufferEvent(event);
	}

	private bufferEvent(event: LiveEventFrame): void {
		const overflow = this.eventBuffer.buffer(event);
		if (overflow) {
			this.failPermanently(overflow);
		}
	}

	private rejectPending(error: Error): void {
		this.frameTransport.rejectAll(error);
		for (const waiter of this.eventWaiters) {
			waiter.reject(error);
		}
		this.eventWaiters.clear();
	}
}
