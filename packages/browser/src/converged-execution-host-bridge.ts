// Execution host bridge for the converged Agent OS wasm sidecar.
//
// Two roles, selected by configuration:
//
// 1. DRIVER mode (default, no `agentExecutor`): mirrors secure-exec's no-op
//    execution host bridge. In the converged browser runtime the guest runs in the
//    browser worker (driven by @rivet-dev/agentos-runtime-browser's runtime driver), not in the
//    wasm sidecar; the sidecar only needs a kernel process (pid) for socket
//    ownership, created by an `execute` wire request. `startExecution` echoes the
//    driver-provided execution id (set via `setNextExecutionId`); the stdio
//    callbacks are no-ops (the driver owns the guest's stdio).
//
// 2. AGENT mode (`agentExecutor` provided): runs a SYNCHRONOUS in-process agent.
//    Each ACP step returns an internal pending response; the production wrapper
//    drains the output queued here and feeds it back on fresh, non-nested
//    `pushFrame` calls. An async Worker cannot be polled synchronously on the main
//    thread, so this bridge intentionally accepts only SyncAgentExecutor.
//
// The wasm `AgentOsBrowserSidecarWasm` host bridge invokes each method with a JSON
// request string and JSON-decodes the return value.

import type { AgentOutput } from "./agent-drive-loop.js";

/** A synchronous ACP agent: each newline-delimited stdin JSON-RPC line maps to
 * zero or more response lines, computed with no async I/O. */
export interface SyncAgent {
	/** Handle one complete stdin line (no trailing newline); return response lines. */
	handleLine(line: string): string[];
}

/** Creates a {@link SyncAgent} for one execution (the launched ACP adapter). */
export interface SyncAgentExecutor {
	createAgent(request: {
		executionId: string;
		vmId: string;
		argv: string[];
		env: Record<string, string>;
		cwd: string;
	}): SyncAgent;
}

export interface ConvergedExecutionHostBridgeOptions {
	/** When set, `startExecution` runs a synchronous in-process agent (AGENT mode). */
	agentExecutor?: SyncAgentExecutor;
}

export interface ConvergedExecutionHostBridge {
	/** Set the execution id `startExecution` echoes next (DRIVER mode). */
	setNextExecutionId(executionId: string): void;
	/** Bind the opaque ACP process handle to the execution most recently spawned
	 * for it. Existing bindings are retained for subsequent session prompts. */
	bindPendingProcess(processId: string): void;
	/** Read output from the exact execution bound to an ACP process handle. */
	pollAgentOutput(processId: string, deadlineMs: number): AgentOutput | null;
	/** The host-bridge object passed to `new AgentOsBrowserSidecarWasm(bridge)`. */
	readonly bridge: Record<string, (requestJson: string) => unknown>;
}

interface AgentSession {
	vmId: string;
	agent: SyncAgent;
	buffer: string;
	events: AgentHostEvent[];
	exited: boolean;
}

interface AgentHostEvent {
	type: "stdout" | "stderr" | "exited";
	vmId?: string;
	executionId?: string;
	chunkBase64?: string;
	exitCode?: number;
}

function decodeBase64(base64: string): Uint8Array {
	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
	return bytes;
}

function decodeBase64ToText(base64: string): string {
	return new TextDecoder().decode(decodeBase64(base64));
}

function encodeTextToBase64(text: string): string {
	const bytes = new TextEncoder().encode(text);
	let binary = "";
	for (const byte of bytes) binary += String.fromCharCode(byte);
	return btoa(binary);
}

export function createConvergedExecutionHostBridge(
	options: ConvergedExecutionHostBridgeOptions = {},
): ConvergedExecutionHostBridge {
	let nextExecutionId = "converged-exec";
	let contextCounter = 0;
	let workerCounter = 0;
	let agentCounter = 0;
	let lastAgentExecutionId: string | null = null;
	const executor = options.agentExecutor;
	const sessions = new Map<string, AgentSession>();
	const pendingProcesses = new Map<string, string>();

	const bridge: Record<string, (requestJson: string) => unknown> = {
		createJavascriptContext() {
			contextCounter += 1;
			return { contextId: `converged-ctx-${contextCounter}` };
		},
		createWasmContext() {
			contextCounter += 1;
			return { contextId: `converged-wasm-ctx-${contextCounter}` };
		},
		startExecution(requestJson: string) {
			if (!executor) {
				// DRIVER mode: echo the driver-provided id.
				return { executionId: nextExecutionId };
			}
			// AGENT mode: mint a fresh id and instantiate the synchronous agent.
			const request = parse(requestJson);
			agentCounter += 1;
			const executionId = `agent-exec-${agentCounter}`;
			lastAgentExecutionId = executionId;
			const vmId = typeof request.vmId === "string" ? request.vmId : "";
			const agent = executor.createAgent({
				executionId,
				vmId,
				argv: Array.isArray(request.argv) ? (request.argv as string[]) : [],
				env:
					request.env && typeof request.env === "object"
						? (request.env as Record<string, string>)
						: {},
				cwd: typeof request.cwd === "string" ? request.cwd : "",
			});
			sessions.set(executionId, {
				vmId,
				agent,
				buffer: "",
				events: [],
				exited: false,
			});
			return { executionId };
		},
		createWorker(requestJson: string) {
			workerCounter += 1;
			const request = parse(requestJson);
			return {
				workerId: `converged-worker-${workerCounter}`,
				runtime: typeof request.runtime === "string" ? request.runtime : undefined,
			};
		},
		writeExecutionStdin(requestJson: string) {
			const request = parse(requestJson);
			const executionId =
				typeof request.executionId === "string" ? request.executionId : "";
			const session = sessions.get(executionId);
			if (!session) return {};
			const chunkBase64 =
				typeof request.chunkBase64 === "string" ? request.chunkBase64 : "";
			session.buffer += decodeBase64ToText(chunkBase64);
			let newlineIndex = session.buffer.indexOf("\n");
			while (newlineIndex >= 0) {
				const line = session.buffer.slice(0, newlineIndex);
				session.buffer = session.buffer.slice(newlineIndex + 1);
				if (line.trim()) {
					for (const output of session.agent.handleLine(line)) {
						session.events.push({
							type: "stdout",
							vmId: session.vmId,
							executionId,
							chunkBase64: encodeTextToBase64(`${output}\n`),
						});
					}
				}
				newlineIndex = session.buffer.indexOf("\n");
			}
			return {};
		},
		closeExecutionStdin() {
			return {};
		},
		killExecution(requestJson: string) {
			const request = parse(requestJson);
			const executionId =
				typeof request.executionId === "string" ? request.executionId : "";
			const session = sessions.get(executionId);
			if (session && !session.exited) {
				session.exited = true;
				session.events.push({
					type: "exited",
					vmId: session.vmId,
					executionId,
					exitCode: 0,
				});
			}
			return {};
		},
		pollExecutionEvent(requestJson: string) {
			const request = parse(requestJson);
			const vmId = typeof request.vmId === "string" ? request.vmId : "";
			for (const session of sessions.values()) {
				if (session.vmId === vmId && session.events.length > 0) {
					return session.events.shift();
				}
			}
			return null;
		},
		terminateWorker(requestJson: string) {
			const request = parse(requestJson);
			const executionId =
				typeof request.executionId === "string" ? request.executionId : "";
			if (executionId.length === 0) return {};
			sessions.delete(executionId);
			for (const [processId, boundExecutionId] of pendingProcesses) {
				if (boundExecutionId === executionId) pendingProcesses.delete(processId);
			}
			if (lastAgentExecutionId === executionId) lastAgentExecutionId = null;
			return {};
		},
		emitStructuredEvent() {
			return {};
		},
		emitDiagnostic() {
			return {};
		},
		emitLog() {
			return {};
		},
		emitLifecycle() {
			return {};
		},
	};

	return {
		setNextExecutionId(executionId: string) {
			nextExecutionId = executionId;
		},
		bindPendingProcess(processId: string) {
			if (pendingProcesses.has(processId)) return;
			if (!lastAgentExecutionId || !sessions.has(lastAgentExecutionId)) {
				throw new Error(
					`cannot bind ACP process ${processId}: no agent execution was spawned`,
				);
			}
			pendingProcesses.set(processId, lastAgentExecutionId);
		},
		pollAgentOutput(
			processId: string,
			_deadlineMs: number,
			isCancelled?: () => boolean,
		) {
			if (isCancelled?.()) return null;
			const executionId = pendingProcesses.get(processId);
			if (!executionId) {
				throw new Error(`unknown ACP process ${processId}`);
			}
			const session = sessions.get(executionId);
			if (!session) {
				throw new Error(`unknown ACP execution ${executionId}`);
			}
			const event = session.events.shift();
			if (!event) return null;
			if (event.type === "exited") {
				pendingProcesses.delete(processId);
				return {
					kind: "exit" as const,
					payload: new TextEncoder().encode(String(event.exitCode ?? 0)),
				};
			}
			return {
				kind: event.type,
				payload: decodeBase64(event.chunkBase64 ?? ""),
			};
		},
		bridge,
	};
}

function parse(requestJson: string): Record<string, unknown> {
	try {
		const value = JSON.parse(requestJson);
		return typeof value === "object" && value !== null
			? (value as Record<string, unknown>)
			: {};
	} catch {
		return {};
	}
}
