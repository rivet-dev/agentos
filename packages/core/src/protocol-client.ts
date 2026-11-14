import type { Readable, Writable } from "node:stream";
import {
	type LiveSidecarEventSelector,
	normalizeSidecarEventMatcher,
	SidecarEventBuffer,
	SidecarEventBufferOverflow,
	sidecarEventWaitAbortError,
} from "./event-buffer.js";
import { FrameRpcTransport } from "./frame-rpc.js";
import {
	type FrameTransport,
	SplitLaneFrameTransport,
	StdioFrameTransport,
} from "./frame-stream.js";
import type { LiveOwnershipScope } from "./ownership.js";
import {
	classifySidecarWrittenProtocolFrame,
	decodeProtocolFramePayload,
	encodeProtocolFramePayload,
	HostProtocolFrameFactory,
	type LiveControlFrame,
	type LiveEventFrame,
	type LiveProtocolFrame,
	type LiveRequestFrame,
	type LiveResponseFrame,
	type LiveSidecarRequestFrame,
	type LiveSidecarRequestHandler,
	type LiveSidecarResponseFrame,
	type ProtocolFramePayloadCodec,
	resolveSidecarRequestFramePayload,
} from "./protocol-frames.js";
import type { LiveRequestPayload } from "./request-payloads.js";
import {
	SidecarRejectedError,
	SidecarSilenceTimeout,
} from "./sidecar-errors.js";

/**
 * How much eligible observed inbound silence (no responses, events,
 * sidecar requests, or heartbeats) the host counts before confirming expiry. The
 * sidecar heartbeats every 10s from a dedicated thread, so this allows two
 * missed beats plus margin; it bounds "sidecar is dead or wedged", never "this
 * request is slow" — individual requests have no deadline of their own.
 */
const DEFAULT_SIDECAR_SILENCE_TIMEOUT_MS = 30_000;

export interface SidecarProtocolClientOptions {
	/** A combined transport remains supported for embedders and focused tests. */
	frameTransport?: FrameTransport<
		LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
		LiveProtocolFrame
	>;
	ordinaryFrameTransport?: FrameTransport<LiveEventFrame, LiveRequestFrame>;
	controlFrameTransport?: FrameTransport<
		LiveResponseFrame | LiveSidecarRequestFrame | LiveEventFrame,
		LiveSidecarResponseFrame | LiveControlFrame
	>;
	stdin?: Writable;
	stdout?: Readable;
	control?: Readable & Writable;
	/** Use one bidirectional stdin/stdout transport when the host cannot inherit fd 3. */
	combinedStdio?: boolean;
	eventBufferCapacity: number;
	payloadCodec?: ProtocolFramePayloadCodec;
	stderrText?: () => string;
	frameError?: (error: Error) => Error;
	/** Classify a failed outbound frame write as terminal, if applicable. */
	fatalWriteError?: (error: Error) => Error | undefined;
	streamEndedError?: () => Error;
	/** Override the silence watchdog window. Tests only; production uses the default. */
	silenceTimeoutMs?: number;
	/**
	 * Runs when the silence watchdog fires, before pending work is rejected.
	 * The stdio layer uses it to SIGKILL the sidecar child.
	 */
	onSilenceExpired?: () => void;
}

export class SidecarProtocolClient {
	private readonly eventBuffer: SidecarEventBuffer<LiveEventFrame>;
	private readonly eventListeners = new Set<(event: LiveEventFrame) => void>();
	private readonly silenceTimeoutMs: number;
	private silenceTimer: ReturnType<typeof setInterval> | null = null;
	private silenceObservedAtMs = 0;
	private silenceCountedMs = 0;
	/** Observing has no pending expiry; confirming owns exactly one cancellable check. */
	private silencePhase:
		| { kind: "observing" }
		| { kind: "confirming"; handle: ReturnType<typeof setImmediate> }
		| { kind: "closed" } = { kind: "observing" };
	private readonly payloadCodec: ProtocolFramePayloadCodec;
	private readonly stderrText: () => string;
	private readonly frameError: (error: Error) => Error;
	private readonly fatalWriteError:
		| ((error: Error) => Error | undefined)
		| undefined;
	private readonly hostFrameFactory = new HostProtocolFrameFactory();
	private ingressRetryRequests = 0;
	private ingressRetryBytes = 0;
	private readonly frameTransport: FrameRpcTransport<
		LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
		LiveRequestFrame | LiveSidecarResponseFrame | LiveControlFrame,
		LiveResponseFrame,
		LiveEventFrame,
		LiveSidecarRequestFrame
	>;
	private closedError: Error | null = null;
	/**
	 * Frame limit from the sidecar's authenticate response. Only the small
	 * authenticate request is written before it is known.
	 */
	private maxFrameBytes: number | undefined;
	private readonly closeListeners = new Set<(error: Error) => void>();
	private readonly eventWaiters = new Set<{
		matches: (event: LiveEventFrame) => boolean;
		resolve: (event: LiveEventFrame) => void;
		reject: (error: Error) => void;
		timer: ReturnType<typeof setTimeout> | null;
	}>();
	private sidecarRequestHandler: LiveSidecarRequestHandler | null = null;

	constructor(options: SidecarProtocolClientOptions) {
		this.silenceTimeoutMs =
			options.silenceTimeoutMs ?? DEFAULT_SIDECAR_SILENCE_TIMEOUT_MS;
		this.eventBuffer = new SidecarEventBuffer(options.eventBufferCapacity);
		this.payloadCodec = options.payloadCodec ?? "bare";
		this.stderrText = options.stderrText ?? (() => "");
		this.frameError = options.frameError ?? ((error) => error);
		this.fatalWriteError = options.fatalWriteError;
		this.frameTransport = new FrameRpcTransport<
			LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
			LiveRequestFrame | LiveSidecarResponseFrame | LiveControlFrame,
			LiveResponseFrame,
			LiveEventFrame,
			LiveSidecarRequestFrame
		>({
			frameTransport: resolveProtocolFrameTransport(
				options,
				this.payloadCodec,
				() => this.maxFrameBytes,
			),
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
			this.failPermanently(this.frameError(error));
		});
		this.frameTransport.onEnd(() => {
			this.failPermanently(
				options.streamEndedError?.() ??
					new Error("sidecar protocol stream ended"),
			);
		});
		this.frameTransport.onFrameActivity(() => {
			this.noteInbound(performance.now());
		});
		this.startSilenceWatchdog(options.onSilenceExpired);
	}

	/** Count at most one interval per observed tick. One check-phase confirmation
	 * gives the existing I/O callbacks a scheduling opportunity, not proof that a
	 * pipe is empty. No hard wall-clock bound is possible while the host stalls.
	 */
	private startSilenceWatchdog(onExpired?: () => void): void {
		if (this.closedError || this.silencePhase.kind === "closed") return;
		this.silenceObservedAtMs = performance.now();
		const checkIntervalMs = Math.max(
			Math.min(this.silenceTimeoutMs / 4, 1_000),
			10,
		);
		this.silenceTimer = setInterval(() => {
			if (this.silencePhase.kind !== "observing") return;
			const now = performance.now();
			this.silenceCountedMs += Math.min(
				Math.max(now - this.silenceObservedAtMs, 0),
				checkIntervalMs,
			);
			this.silenceObservedAtMs = Math.max(now, this.silenceObservedAtMs);
			if (this.silenceCountedMs < this.silenceTimeoutMs) return;
			const confirmation = setImmediate(() => {
				if (this.silencePhase !== pending) return;
				const error = new SidecarSilenceTimeout({
					silenceMs: this.silenceCountedMs,
					stderr: this.stderrText(),
				});
				// Publish Closed before any reentrant operator callback or rejection.
				this.closedError = error;
				this.stopSilenceWatchdog();
				try {
					onExpired?.();
				} finally {
					this.finishFailure(this.closedError ?? error, true);
				}
			});
			const pending = { kind: "confirming" as const, handle: confirmation };
			this.silencePhase = pending;
		}, checkIntervalMs);
		this.silenceTimer.unref?.();
	}

	private noteInbound(now: number): void {
		if (this.silencePhase.kind === "closed") return;
		if (this.silencePhase.kind === "confirming")
			clearImmediate(this.silencePhase.handle);
		this.silencePhase = { kind: "observing" };
		this.silenceObservedAtMs = Math.max(this.silenceObservedAtMs, now);
		this.silenceCountedMs = 0;
	}

	private stopSilenceWatchdog(): void {
		if (this.silencePhase.kind === "confirming")
			clearImmediate(this.silencePhase.handle);
		this.silencePhase = { kind: "closed" };
		if (this.silenceTimer !== null) {
			clearInterval(this.silenceTimer);
			this.silenceTimer = null;
		}
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

	/** Runs `handler` once when the connection to the sidecar fails for good. */
	onClose(handler: (error: Error) => void): () => void {
		if (this.closedError) {
			handler(this.closedError);
			return () => {};
		}
		this.closeListeners.add(handler);
		return () => {
			this.closeListeners.delete(handler);
		};
	}

	async sendRequest(input: {
		ownership: LiveOwnershipScope;
		payload: LiveRequestPayload;
	}): Promise<LiveResponseFrame> {
		if (this.closedError) throw this.closedError;
		const original = this.hostFrameFactory.createRequestFrame(input);
		const retainedBytes = encodeProtocolFramePayload(original, this.payloadCodec).byteLength;
		// Preserve the exact logical request, including binary arguments, while
		// retrying only a typed refusal issued before ordinary ingress admission.
		const request = structuredClone(original);
		let retryReserved = false;
		try {
			for (let attempt = 0; ; attempt++) {
				if (this.closedError) throw this.closedError;
				let response: LiveResponseFrame;
				try { response = await this.frameTransport.sendFrame(request.request_id, request); }
				catch (error) {
					const fatal = this.failOnFatalWrite(error);
					if (fatal) throw fatal;
					throw error;
				}
				if (response.payload.type !== "rejected") {
					if (response.payload.type === "authenticated") this.maxFrameBytes = response.payload.max_frame_bytes;
					return response;
				}
				const detail = response.payload;
				const ingressPressure = detail.code === "ERR_AGENTOS_PROTOCOL_FRAME_LIMIT" || detail.code === "ERR_AGENTOS_PROTOCOL_BYTE_LIMIT";
				const canRetry = ingressPressure && detail.retryable === true && detail.errno === "EAGAIN"
					&& detail.operation === "stdio.requestAdmission" && detail.limit_name === "stdio ordinary ingress"
					&& (detail.configuration_path === "runtime.protocol.maxIngressFrames" || detail.configuration_path === "runtime.protocol.maxIngressBytes")
					&& detail.requested !== null && detail.configured_limit !== null && detail.requested <= detail.configured_limit;
				if (!canRetry || attempt >= 32) throw new SidecarRejectedError(request.request_id, detail);
				if (!retryReserved) {
					// Admission retries cannot create an unbounded host timer/payload
					// queue. Exhaustion preserves the original typed refusal.
					if (this.ingressRetryRequests >= 4096 || retainedBytes > 64 * 1024 * 1024 - this.ingressRetryBytes) throw new SidecarRejectedError(request.request_id, detail);
					this.ingressRetryRequests++; this.ingressRetryBytes += retainedBytes; retryReserved = true;
				}
				await this.waitForIngressRetry(Math.min(2 ** Math.min(attempt + 1, 7), 100) + request.request_id % 17);
			}
		} finally {
			if (retryReserved) { this.ingressRetryRequests--; this.ingressRetryBytes -= retainedBytes; }
		}
	}

	private waitForIngressRetry(milliseconds: number): Promise<void> {
		return new Promise((resolve, reject) => {
			let unsubscribe = () => {};
			const timer = setTimeout(() => { unsubscribe(); resolve(); }, milliseconds);
			unsubscribe = this.onClose(error => { clearTimeout(timer); unsubscribe(); reject(error); });
		});
	}

	async shutdown(reason: string): Promise<void> {
		if (this.closedError) {
			throw this.closedError;
		}
		await this.writeFrame(
			this.hostFrameFactory.createControlFrame({ type: "shutdown", reason }),
		);
	}

	async waitForEvent(
		matcher: LiveSidecarEventSelector | ((event: LiveEventFrame) => boolean),
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
					if (waiter.confirmation !== null) {
						clearImmediate(waiter.confirmation);
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
					if (waiter.confirmation !== null) {
						clearImmediate(waiter.confirmation);
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
								waiter.confirmation = setImmediate(() => {
									if (!this.eventWaiters.has(waiter)) return;
									waiter.reject(
										new Error(
											`timed out waiting for sidecar event\nstderr:\n${this.stderrText()}`,
										),
									);
								});
							}, timeoutMs),
				confirmation: null as ReturnType<typeof setImmediate> | null,
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
		const firstFailure = this.closedError === null;
		this.closedError = error;
		this.stopSilenceWatchdog();
		this.finishFailure(error, firstFailure);
	}

	private finishFailure(error: Error, firstFailure: boolean): void {
		this.rejectPending(error);
		if (firstFailure) {
			for (const handler of this.closeListeners) {
				handler(error);
			}
			this.closeListeners.clear();
		}
	}

	dispose(): void {
		this.failPermanently(new Error("sidecar protocol client disposed"));
		this.frameTransport.dispose();
	}

	private failOnFatalWrite(error: unknown): Error | undefined {
		const normalized =
			error instanceof Error ? error : new Error(String(error));
		const fatal = this.fatalWriteError?.(normalized);
		if (!fatal) return undefined;
		// A write callback can report a broken pipe before the stream or child
		// emits its terminal event. Close the protocol immediately in that case.
		this.failPermanently(fatal);
		return this.closedError ?? fatal;
	}

	private async writeFrame(
		frame: LiveSidecarResponseFrame | LiveControlFrame,
	): Promise<void> {
		try {
			await this.frameTransport.writeFrame(frame);
		} catch (error) {
			throw this.failOnFatalWrite(error) ?? error;
		}
	}

	private async dispatchSidecarRequest(
		request: LiveSidecarRequestFrame,
	): Promise<void> {
		// A callback can arrive immediately before the response that acknowledges
		// the command which created it. Let the inbound decoder finish that turn
		// before writing a callback response; otherwise a very fast host callback
		// can complete and reap the guest process while its execute request is
		// still being admitted, leaving the host waiting for an exit it missed.
		await new Promise<void>((resolve) => {
			const scheduleImmediate = (
				globalThis as typeof globalThis & {
					setImmediate?: (callback: () => void) => unknown;
				}
			).setImmediate;
			if (scheduleImmediate) {
				scheduleImmediate(resolve);
			} else {
				setTimeout(resolve, 0);
			}
		});
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
		// Transport-level liveness beats from the sidecar. Their arrival already
		// reset the silence watchdog at the frame layer; they carry no meaning
		// for consumers and must never reach the bounded event buffer, where a
		// long-idle VM would accumulate one every 10s until overflow.
		if (
			event.payload.type === "structured" &&
			event.payload.name === "heartbeat"
		) {
			return;
		}
		for (const listener of this.eventListeners) {
			try {
				const result: unknown = listener(event);
				// Observe rejected callback results without delaying event delivery.
				if (result !== undefined) {
					void Promise.resolve(result).catch(reportEventObserverError);
				}
			} catch (error) {
				reportEventObserverError(error);
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

function reportEventObserverError(error: unknown): void {
	console.error(
		"ERR_AGENTOS_SIDECAR_EVENT_OBSERVER: event observer failed",
		error,
	);
}

function resolveProtocolFrameTransport(
	options: SidecarProtocolClientOptions,
	payloadCodec: ProtocolFramePayloadCodec,
	maxFrameBytes: () => number | undefined,
): FrameTransport<
	LiveResponseFrame | LiveEventFrame | LiveSidecarRequestFrame,
	LiveRequestFrame | LiveSidecarResponseFrame | LiveControlFrame
> {
	const hasSplitTransport =
		options.ordinaryFrameTransport !== undefined ||
		options.controlFrameTransport !== undefined;
	const hasStdio =
		options.stdin !== undefined ||
		options.stdout !== undefined ||
		options.control !== undefined;
	if (options.combinedStdio) {
		if (
			options.frameTransport ||
			hasSplitTransport ||
			options.control !== undefined ||
			!options.stdin ||
			!options.stdout
		) {
			throw new Error(
				"SidecarProtocolClient combinedStdio requires only stdin and stdout streams",
			);
		}
		return new StdioFrameTransport({
			stdin: options.stdin,
			stdout: options.stdout,
			encodeFrame: (frame) => encodeProtocolFramePayload(frame, payloadCodec),
			maxFrameBytes,
			decodeFrame: (payload) =>
				decodeProtocolFramePayload(payload, payloadCodec) as
					| LiveResponseFrame
					| LiveEventFrame
					| LiveSidecarRequestFrame,
		});
	}
	if (options.frameTransport) {
		if (hasSplitTransport || hasStdio) {
			throw new Error(
				"SidecarProtocolClient frameTransport cannot be combined with split-lane transports or streams",
			);
		}
		return options.frameTransport;
	}

	let ordinary = options.ordinaryFrameTransport;
	let control = options.controlFrameTransport;
	if (hasSplitTransport) {
		if (!ordinary || !control || hasStdio) {
			throw new Error(
				"SidecarProtocolClient requires both ordinaryFrameTransport and controlFrameTransport, without stdio streams",
			);
		}
	} else {
		if (!options.stdin || !options.stdout || !options.control) {
			throw new Error(
				"SidecarProtocolClient requires a combined frameTransport or stdin/stdout/control streams",
			);
		}
		ordinary = new StdioFrameTransport<LiveEventFrame, LiveRequestFrame>({
			stdin: options.stdin,
			stdout: options.stdout,
			encodeFrame: (frame) => encodeProtocolFramePayload(frame, payloadCodec),
			maxFrameBytes,
			decodeFrame: (payload) => {
				const frame = decodeProtocolFramePayload(payload, payloadCodec);
				validateOrdinaryInboundFrame(frame);
				return frame;
			},
		});
		control = new StdioFrameTransport<
			LiveResponseFrame | LiveSidecarRequestFrame | LiveEventFrame,
			LiveSidecarResponseFrame | LiveControlFrame
		>({
			stdin: options.control,
			stdout: options.control,
			encodeFrame: (frame) => encodeProtocolFramePayload(frame, payloadCodec),
			maxFrameBytes,
			decodeFrame: (payload) => {
				const frame = decodeProtocolFramePayload(payload, payloadCodec);
				validateControlInboundFrame(frame);
				return frame;
			},
		});
	}

	return new SplitLaneFrameTransport({
		ordinary,
		control,
		validateOrdinaryFrame: validateOrdinaryInboundFrame,
		validateControlFrame: validateControlInboundFrame,
		selectWriteLane: (frame) => {
			switch (frame.frame_type) {
				case "request":
					return "ordinary";
				case "sidecar_response":
				case "control":
					return "control";
				default: {
					const invalidFrame = frame as { frame_type: string };
					throw new Error(
						`host frame ${invalidFrame.frame_type} is not valid on either writable protocol lane`,
					);
				}
			}
		},
	});
}

function validateOrdinaryInboundFrame(frame: {
	frame_type: string;
	payload?: unknown;
}): asserts frame is LiveEventFrame {
	if (frame.frame_type !== "event") {
		throw new Error(
			`sidecar frame ${frame.frame_type} is not valid on the ordinary protocol lane`,
		);
	}
	if (isHeartbeatFrame(frame as LiveEventFrame)) {
		throw new Error(
			"sidecar heartbeat event is not valid on the ordinary protocol lane",
		);
	}
}

function validateControlInboundFrame(frame: {
	frame_type: string;
	payload?: unknown;
}): asserts frame is
	| LiveResponseFrame
	| LiveSidecarRequestFrame
	| LiveEventFrame {
	if (
		frame.frame_type === "response" ||
		frame.frame_type === "sidecar_request"
	) {
		return;
	}
	if (
		frame.frame_type === "event" &&
		isHeartbeatFrame(frame as LiveEventFrame)
	) {
		return;
	}
	throw new Error(
		`sidecar frame ${frame.frame_type} is not valid on the control protocol lane`,
	);
}

function isHeartbeatFrame(frame: LiveEventFrame): boolean {
	return (
		frame.payload.type === "structured" && frame.payload.name === "heartbeat"
	);
}
