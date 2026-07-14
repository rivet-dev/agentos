import { resolvePublishedSidecarBinary } from "./binary.js";
import {
	SidecarProcessExited,
	StdioSidecarProcess,
} from "./process.js";
import { SidecarProtocolClient } from "./protocol-client.js";
import type { SidecarProcessTransport } from "./sidecar-client.js";
import { registerSidecarProcessSpawnFactory } from "./sidecar-process.js";
import type {
	LiveEventFrame,
	LiveResponseFrame,
	LiveSidecarRequestHandler,
	ProtocolFramePayloadCodec,
} from "./protocol-frames.js";
import type { LiveOwnershipScope } from "./ownership.js";
import type { LiveRequestPayload } from "./request-payloads.js";
import type { LiveSidecarEventSelector } from "./event-buffer.js";

export const DEFAULT_SIDECAR_EVENT_BUFFER_CAPACITY = 4_096;
export const DEFAULT_SIDECAR_GRACEFUL_EXIT_MS = 5_000;
export const DEFAULT_SIDECAR_FORCE_EXIT_MS = 2_000;

export interface StdioSidecarProtocolClientSpawnOptions {
	cwd?: string;
	command?: string;
	args?: string[];
	eventBufferCapacity?: number;
	gracefulExitMs?: number;
	forceExitMs?: number;
	disposedErrorMessage?: string;
	payloadCodec?: ProtocolFramePayloadCodec;
	/**
	 * Override the silence watchdog window (default 30s). Tests only — the
	 * window is a fixed protocol constant paired with the sidecar's 10s
	 * heartbeat cadence, not an operator tunable.
	 */
	silenceTimeoutMs?: number;
}

type ResolvedStdioSidecarProtocolClientOptions = Required<
	Pick<
		StdioSidecarProtocolClientSpawnOptions,
		| "eventBufferCapacity"
		| "gracefulExitMs"
		| "forceExitMs"
		| "disposedErrorMessage"
		| "payloadCodec"
	>
> &
	Pick<StdioSidecarProtocolClientSpawnOptions, "silenceTimeoutMs">;

export class StdioSidecarProtocolClient implements SidecarProcessTransport {
	readonly child: StdioSidecarProcess["child"];
	private readonly sidecarProcess: StdioSidecarProcess;
	private readonly protocolClient: SidecarProtocolClient;
	private readonly gracefulExitMs: number;
	private readonly forceExitMs: number;
	private readonly disposedErrorMessage: string;

	private constructor(
		sidecarProcess: StdioSidecarProcess,
		options: ResolvedStdioSidecarProtocolClientOptions,
	) {
		this.sidecarProcess = sidecarProcess;
		this.child = sidecarProcess.child;
		this.gracefulExitMs = options.gracefulExitMs;
		this.forceExitMs = options.forceExitMs;
		this.disposedErrorMessage = options.disposedErrorMessage;
		this.protocolClient = new SidecarProtocolClient({
			stdin: this.child.stdin,
			stdout: this.child.stdout,
			eventBufferCapacity: options.eventBufferCapacity,
			payloadCodec: options.payloadCodec,
			silenceTimeoutMs: options.silenceTimeoutMs,
			// A silent sidecar is dead or wedged; reap the process so it cannot
			// linger as a zombie holding VM resources. The watchdog then rejects
			// all in-flight requests with `SidecarSilenceTimeout`.
			onSilenceExpired: () => {
				try {
					this.child.kill("SIGKILL");
				} catch {
					// The child may have exited between the check and the kill.
				}
			},
			stderrText: () => this.sidecarProcess.stderrText(),
			streamEndedError: () =>
				this.sidecarProcess.currentExitError() ??
				new SidecarProcessExited({
					exitCode: this.child.exitCode,
					signal: this.child.signalCode,
					stderr: this.sidecarProcess.stderrText(),
				}),
			frameError: (error) => this.sidecarProcess.currentExitError() ?? error,
		});
		this.sidecarProcess.onExit((error) => {
			this.failPermanently(error);
		});
		this.sidecarProcess.onError((error) => {
			this.failPermanently(error);
		});
	}

	static spawn(
		options: StdioSidecarProtocolClientSpawnOptions = {},
	): StdioSidecarProtocolClient {
		return new StdioSidecarProtocolClient(
			StdioSidecarProcess.spawn({
				command: options.command ?? resolvePublishedSidecarBinary(),
				args: options.args ?? [],
				cwd: options.cwd,
			}),
			{
				silenceTimeoutMs: options.silenceTimeoutMs,
				eventBufferCapacity:
					options.eventBufferCapacity ??
					DEFAULT_SIDECAR_EVENT_BUFFER_CAPACITY,
				gracefulExitMs:
					options.gracefulExitMs ?? DEFAULT_SIDECAR_GRACEFUL_EXIT_MS,
				forceExitMs: options.forceExitMs ?? DEFAULT_SIDECAR_FORCE_EXIT_MS,
				disposedErrorMessage:
					options.disposedErrorMessage ?? "sidecar client disposed",
				payloadCodec: options.payloadCodec ?? "bare",
			},
		);
	}

	registerSidecarRequestHandler(
		ownership: LiveOwnershipScope,
		handler: LiveSidecarRequestHandler,
	): () => void {
		return this.protocolClient.registerSidecarRequestHandler(
			ownership,
			handler,
		);
	}

	onEvent(
		handler: (event: LiveEventFrame) => void,
		ownership?: LiveOwnershipScope,
	): () => void {
		return this.protocolClient.onEvent(handler, ownership);
	}

	async sendRequest(input: {
		ownership: LiveOwnershipScope;
		payload: LiveRequestPayload;
		onResponse?: (response: LiveResponseFrame) => void;
	}): Promise<LiveResponseFrame> {
		return await this.protocolClient.sendRequest(input);
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
		return await this.protocolClient.waitForEvent(matcher, timeoutMs, options);
	}

	async dispose(): Promise<void> {
		this.protocolClient.failPermanently(new Error(this.disposedErrorMessage));

		if (!this.child.stdin.destroyed) {
			try {
				this.child.stdin.end();
			} catch {
				// Stdin may already be closing. The child exit watcher will catch up.
			}
		}

		const exitCode = await this.sidecarProcess.waitForExit(
			this.gracefulExitMs,
		);
		if (exitCode === null) {
			try {
				this.child.kill("SIGKILL");
			} catch {
				// The child may have exited between the timeout and the kill attempt.
			}
			await this.sidecarProcess.waitForExit(this.forceExitMs);
		}

		this.protocolClient.dispose();
		try {
			this.child.stdin.destroy();
		} catch {
			// Best effort. The child is gone so the descriptor will close on its own.
		}
		try {
			this.child.stdout.destroy();
		} catch {
			// Best effort. The child is gone so the descriptor will close on its own.
		}
		try {
			this.child.stderr.destroy();
		} catch {
			// Best effort. The child is gone so the descriptor will close on its own.
		}

		if (
			exitCode !== null &&
			exitCode !== 0 &&
			this.child.signalCode === null
		) {
			throw new Error(
				`sidecar exited with code ${exitCode}\nstderr:\n${this.sidecarProcess.stderrText()}`,
			);
		}
	}

	failPermanently(error: Error): void {
		this.protocolClient.failPermanently(error, {
			replaceExisting: (current, next) =>
				current instanceof SidecarProcessExited &&
				current.exitCode === null &&
				current.signal === null &&
				next instanceof SidecarProcessExited &&
				(next.exitCode !== null || next.signal !== null),
		});
	}
}

registerSidecarProcessSpawnFactory((options) =>
	StdioSidecarProtocolClient.spawn(options),
);
