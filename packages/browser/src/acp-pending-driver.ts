import {
	AgentInteractionError,
	type AgentOutput,
	driveAgentInteraction,
} from "./agent-drive-loop.js";

export interface AcpPendingResponseHost {
	bindPendingProcess(processId: string): void;
	pollAgentOutput(
		processId: string,
		deadlineMs: number,
		isCancelled?: () => boolean,
	): AgentOutput | null;
}

/** Sidecar-owned ACP/frame codec helpers. The browser routing loop deliberately
 * treats both protocol layers as opaque bytes. */
export interface AcpPendingFrameHelpers {
	pendingResponseProcessId(frame: Uint8Array): string | null;
	pendingResponseTimeoutMs(frame: Uint8Array): number | null;
	pendingResponseTimeoutPhase(frame: Uint8Array): string | null;
	buildDeliverAgentOutputFrame(
		originResponse: Uint8Array,
		processId: string,
		chunk: Uint8Array,
	): Uint8Array;
	buildDeliverAgentStderrFrame(
		originResponse: Uint8Array,
		processId: string,
		chunk: Uint8Array,
	): Uint8Array;
	buildAbortPendingFrame(
		originResponse: Uint8Array,
		processId: string,
		reason:
			| "agent_exited"
			| "interaction_timeout"
			| "driver_failed"
			| "caller_cancelled",
		exitCode: number | null,
	): Uint8Array;
	restorePendingResponse(
		originResponse: Uint8Array,
		completedResponse: Uint8Array,
	): Uint8Array;
}

export interface AcpPendingResponseDriverOptions {
	pushFrame(frame: Uint8Array): Uint8Array;
	frameHelpers: AcpPendingFrameHelpers;
	host: AcpPendingResponseHost;
	now?: () => number;
	/** Host-visible bounded diagnostic for driver faults that the sidecar's stable
	 * cleanup response intentionally does not echo. */
	onDriverError?: (processId: string, message: string) => void;
	/** Host cancellation probe. For a blocking worker host this should read shared
	 * state that another realm can update while the interaction is in flight. */
	isCancelled?: (processId: string) => boolean;
}

/**
 * Wrap a browser sidecar's synchronous pushFrame boundary so internal resumable
 * responses never reach ordinary AgentOs callers. TypeScript only routes opaque
 * frames and agent output; Rust owns ACP and outer-frame serialization.
 */
export function createAcpPendingResponseDriver(
	options: AcpPendingResponseDriverOptions,
): (frame: Uint8Array) => Uint8Array {
	const now = options.now ?? Date.now;

	return (frame) => {
		const originResponse = options.pushFrame(frame);
		let pendingResponse = originResponse;
		let processId =
			options.frameHelpers.pendingResponseProcessId(pendingResponse);
		if (processId === null) return pendingResponse;

		let completed: Uint8Array | null = null;
		while (completed === null) {
			try {
				if (options.isCancelled?.(processId)) {
					throw new AgentInteractionError(
						`agent interaction was cancelled (${processId})`,
						"caller_cancelled",
					);
				}
				const activeProcessId = processId;
				const timeoutMs =
					options.frameHelpers.pendingResponseTimeoutMs(pendingResponse);
				const timeoutPhase =
					options.frameHelpers.pendingResponseTimeoutPhase(pendingResponse);
				if (timeoutMs === null || timeoutPhase === null) {
					throw new Error(
						"ACP pending response omitted its sidecar timeout phase",
					);
				}
				options.host.bindPendingProcess(activeProcessId);
				completed = driveAgentInteraction(
					{
						now,
						isCancelled: () =>
							options.isCancelled?.(activeProcessId) ?? false,
						pollAgentOutput: (pendingProcessId, deadlineMs) =>
							options.host.pollAgentOutput(
								pendingProcessId,
								deadlineMs,
								() => options.isCancelled?.(pendingProcessId) ?? false,
							),
						onAgentStderr: (pendingProcessId, chunk) => {
							const response = options.pushFrame(
								options.frameHelpers.buildDeliverAgentStderrFrame(
									originResponse,
									pendingProcessId,
									chunk,
								),
							);
							if (
								options.frameHelpers.pendingResponseProcessId(response) !== null
							) {
								throw new Error("ACP stderr delivery returned a pending response");
							}
						},
						deliverAgentOutput: (pendingProcessId, chunk) => {
							const response = options.pushFrame(
								options.frameHelpers.buildDeliverAgentOutputFrame(
									originResponse,
									pendingProcessId,
									chunk,
								),
							);
							const nextProcessId =
								options.frameHelpers.pendingResponseProcessId(response);
							if (
								nextProcessId !== null &&
								nextProcessId !== pendingProcessId
							) {
								throw new Error(
									`ACP pending process changed from ${pendingProcessId} to ${nextProcessId}`,
								);
							}
							if (nextProcessId === null) return { pending: false, response };
							const nextTimeoutMs =
								options.frameHelpers.pendingResponseTimeoutMs(response);
							if (nextTimeoutMs === null) {
								throw new Error(
									"ACP pending response omitted its sidecar timeout",
								);
							}
							const nextTimeoutPhase =
								options.frameHelpers.pendingResponseTimeoutPhase(response);
							if (nextTimeoutPhase === null) {
								throw new Error(
									"ACP pending response omitted its sidecar timeout phase",
								);
							}
							return {
								pending: true,
								response,
								timeoutMs: nextTimeoutMs,
								timeoutPhase: nextTimeoutPhase,
							};
						},
					},
					activeProcessId,
					now() + timeoutMs,
					timeoutPhase,
				);
			} catch (error) {
				if (!(error instanceof AgentInteractionError)) {
					const message = boundedDriverError(error);
					if (options.onDriverError) {
						options.onDriverError(processId, message);
					} else {
						console.error(`ACP driver failure (${processId}): ${message}`);
					}
				}
				try {
					const abortResponse = options.pushFrame(
						options.frameHelpers.buildAbortPendingFrame(
							originResponse,
							processId,
							error instanceof AgentInteractionError
								? error.reason
								: "driver_failed",
							error instanceof AgentInteractionError ? error.exitCode : null,
						),
					);
					const replacementProcessId =
						options.frameHelpers.pendingResponseProcessId(abortResponse);
					if (replacementProcessId !== null) {
						// Rust core alone decides whether an exited adapter is
						// restartable. TypeScript merely drives the opaque continuation
						// it was handed, exactly as it drives create/resume/prompt.
						pendingResponse = abortResponse;
						processId = replacementProcessId;
						continue;
					}
					completed = abortResponse;
				} catch (cleanupError) {
					throw new AggregateError(
						[error, cleanupError],
						`ACP driver failed and cleanup also failed (${processId})`,
					);
				}
			}
		}
		return options.frameHelpers.restorePendingResponse(
			originResponse,
			completed,
		);
	};
}

const MAX_DRIVER_ERROR_CHARS = 2_048;

function boundedDriverError(error: unknown): string {
	const message = error instanceof Error ? error.message : String(error);
	return message.length <= MAX_DRIVER_ERROR_CHARS
		? message
		: `${message.slice(0, MAX_DRIVER_ERROR_CHARS)}…`;
}
