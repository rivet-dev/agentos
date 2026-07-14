// The kernel-worker drive loop for a resumable ACP interaction (AGENTOS-WEB-ASYNC-
// AGENTS.md §3.2.1). After a create_session / session/prompt wire frame returns
// AcpPending{processId}, the kernel worker drives the agent to completion: it reads
// the agent's stdout from the reactor (which services the agent's own syscalls in
// the meantime — legal, because we are NOT inside a pushFrame here) and feeds each
// chunk back via deliver_agent_output (a fresh, non-nested pushFrame) until the
// handshake/turn completes and yields the real result.
//
// This is pure orchestration over injected dependencies, so it is unit-testable
// without the wasm kernel, the reactor, or real workers.

export type AgentOutputKind = "stdout" | "stderr" | "exit";

export interface AgentOutput {
	kind: AgentOutputKind;
	payload: Uint8Array;
}

export interface DriveResult {
	/** True while the interaction is still in flight (AcpPendingResponse). */
	pending: boolean;
	/** The decoded response frame bytes (the real ACP result when !pending). */
	response: Uint8Array;
	/** Sidecar-owned deadline for the next pending ACP phase. */
	timeoutMs?: number;
	/** Stable identity for the phase that owns `timeoutMs`. */
	timeoutPhase?: string;
}

export interface AgentDriveDeps {
	/** Read the next output for this execution from the reactor, servicing the
	 * agent's syscalls while waiting; null on timeout. */
	pollAgentOutput(
		processId: string,
		deadlineMs: number,
		isCancelled?: () => boolean,
	): AgentOutput | null;
	/** Feed a chunk of agent stdout into the resumable handshake (a non-nested
	 * pushFrame) and return whether it is still pending + the response frame. */
	deliverAgentOutput(processId: string, chunk: Uint8Array): DriveResult;
	/** Surface adapter diagnostics. When omitted, the driver warns to the console
	 * rather than silently discarding stderr. */
	onAgentStderr?(processId: string, chunk: Uint8Array): void;
	/** Cancellation seam for a worker host; may read an Atomics-backed flag. */
	isCancelled?: () => boolean;
	now(): number;
}

export class AgentInteractionError extends Error {
	constructor(
		message: string,
		readonly reason:
			| "agent_exited"
			| "interaction_timeout"
			| "caller_cancelled",
		readonly exitCode: number | null = null,
	) {
		super(message);
		this.name = "AgentInteractionError";
	}
}

/**
 * Drive a resumable ACP interaction to completion. Returns the real ACP result
 * frame bytes (the deferred response the kernel worker relays to the client).
 * Throws if the agent exits or times out before producing a result.
 */
export function driveAgentInteraction(
	deps: AgentDriveDeps,
	processId: string,
	deadlineMs: number,
	timeoutPhase: string,
): Uint8Array {
	for (;;) {
		if (deps.isCancelled?.()) {
			throw new AgentInteractionError(
				`agent interaction was cancelled (${processId})`,
				"caller_cancelled",
			);
		}
		if (deps.now() >= deadlineMs) {
			throw new AgentInteractionError(
				`agent interaction timed out (${processId})`,
				"interaction_timeout",
			);
		}
		const out = deps.pollAgentOutput(processId, deadlineMs, deps.isCancelled);
		if (deps.isCancelled?.()) {
			throw new AgentInteractionError(
				`agent interaction was cancelled (${processId})`,
				"caller_cancelled",
			);
		}
		if (out === null) {
			throw new AgentInteractionError(
				`agent produced no output before the deadline (${processId})`,
				"interaction_timeout",
			);
		}
		if (out.kind === "exit") {
			const exitCode = decodeExitCode(out.payload);
			throw new AgentInteractionError(
				`agent exited before completing the ACP interaction (${processId})`,
				"agent_exited",
				exitCode,
			);
		}
		if (out.kind === "stderr") {
			if (deps.onAgentStderr) {
				deps.onAgentStderr(processId, out.payload);
			} else {
				console.warn(
					`ACP agent stderr (${processId}): ${new TextDecoder().decode(out.payload)}`,
				);
			}
			continue;
		}
		const result = deps.deliverAgentOutput(processId, out.payload);
		if (!result.pending) {
			return result.response;
		}
		if (
			result.timeoutMs === undefined ||
			!Number.isFinite(result.timeoutMs) ||
			result.timeoutMs <= 0 ||
			typeof result.timeoutPhase !== "string" ||
			result.timeoutPhase.length === 0
		) {
			throw new Error(
				"ACP pending response omitted a valid sidecar timeout phase",
			);
		}
		if (result.timeoutPhase !== timeoutPhase) {
			timeoutPhase = result.timeoutPhase;
			deadlineMs = deps.now() + result.timeoutMs;
		}
	}
}

function decodeExitCode(payload: Uint8Array): number | null {
	if (payload.byteLength === 0 || payload.byteLength > 11) return null;
	const text = new TextDecoder().decode(payload);
	if (!/^-?\d+$/.test(text)) return null;
	const value = Number(text);
	return Number.isInteger(value) &&
		value >= -2_147_483_648 &&
		value <= 2_147_483_647
		? value
		: null;
}
