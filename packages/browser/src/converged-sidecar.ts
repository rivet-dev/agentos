// Agent OS converged sidecar loader.
//
// The converged browser runtime lives in `@rivet-dev/agentos-runtime-browser`: the worker, the
// SharedArrayBuffer sync-bridge, and the fs/net/dns/module servicers are all
// reused verbatim. Agent OS plugs in its OWN wasm sidecar — the one that registers
// `BrowserAcpExtension` — via `createBrowserRuntimeDriverFactory({ convergedSidecar })`.
// This is the Agent OS analogue of secure-exec's `createDefaultConvergedSidecar`:
// same `ConvergedSidecarFactoryOptions` contract, different (ACP-bearing) wasm.
//
// The kernel (wasm) remains the sole enforcement point; no guest-side permission
// eval, one transport. The agentos wasm-bindgen web output fetches its own
// `_bg.wasm`; both URLs resolve relative to this module so a consumer's bundler
// (or native ESM loader) emits/serves them.

import type {
	ConvergedSidecarFactoryOptions,
	ConvergedSidecarHandle,
} from "@rivet-dev/agentos-runtime-browser";
import type { ProtocolFramePayloadCodec } from "@rivet-dev/agentos-runtime-core/protocol-frames";
import type { CreateVmConfig } from "@rivet-dev/agentos-runtime-core/vm-config";
import { createAcpPendingResponseDriver } from "./acp-pending-driver.js";
import {
	createConvergedExecutionHostBridge,
	type SyncAgentExecutor,
} from "./converged-execution-host-bridge.js";

const WASM_MODULE_URL = new URL(
	"./sidecar-wasm-web/agentos_sidecar_browser.js",
	import.meta.url,
);
const WASM_BINARY_URL = new URL(
	"./sidecar-wasm-web/agentos_sidecar_browser_bg.wasm",
	import.meta.url,
);

interface AgentOsSidecarWasmWebModule {
	default(input?: unknown): Promise<unknown>;
	AgentOsBrowserSidecarWasm: new (
		hostBridge?: unknown,
	) => {
		pushFrame(frame: Uint8Array): unknown;
		pendingResponseProcessId(frame: Uint8Array): unknown;
		pendingResponseTimeoutMs(frame: Uint8Array): unknown;
		pendingResponseTimeoutPhase(frame: Uint8Array): unknown;
		buildDeliverAgentOutputFrame(
			originResponse: Uint8Array,
			processId: string,
			chunk: Uint8Array,
		): unknown;
		buildDeliverAgentStderrFrame(
			originResponse: Uint8Array,
			processId: string,
			chunk: Uint8Array,
		): unknown;
		buildAbortPendingFrame(
			originResponse: Uint8Array,
			processId: string,
			reason:
				| "agent_exited"
				| "interaction_timeout"
				| "driver_failed"
				| "caller_cancelled",
			exitCode: number | null,
		): unknown;
		restorePendingResponse(
			originResponse: Uint8Array,
			completedResponse: Uint8Array,
		): unknown;
	};
}

export interface AgentOsConvergedSidecarOptions {
	/** Wire codec; defaults to the same-version BARE codec. */
	codec?: ProtocolFramePayloadCodec;
	/** Invoked when the kernel denies a guest fs read with EACCES. */
	onFsReadDenied?: () => void;
	/**
	 * Override the wasm glue-module URL (advanced; defaults to the bundled
	 * dist/sidecar-wasm-web output resolved relative to this module).
	 */
	moduleUrl?: URL | string;
	/** Override the wasm binary URL (advanced; see `moduleUrl`). */
	binaryUrl?: URL | string;
	/**
	 * Run a SYNCHRONOUS in-process ACP agent for `create_session` (e.g. an ACP
	 * echo/test adapter). Required to drive an agent process in-browser, because
	 * the synchronous main-thread `AcpCore` cannot block-wait on an async agent
	 * worker (see AGENTOS-WEB-CONVERGENCE.md Step 7). Omit for guest-only use.
	 */
	agentExecutor?: SyncAgentExecutor;
	/** Probe host-owned cancellation state while an ACP interaction is pending.
	 * Worker integrations should read an Atomics-backed flag so cancellation can
	 * be observed while the kernel worker is synchronously polling its reactor. */
	isAgentInteractionCancelled?: (processId: string) => boolean;
	/** Complete packed `.aospkg` artifacts to forward opaquely into the browser
	 * sidecar during VM initialization. TypeScript never decodes their metadata. */
	packageBytes?: readonly Uint8Array[];
	/** Optional guest package projection root forwarded unchanged to the sidecar. */
	packagesMountAt?: string;
}

/**
 * Build the {@link ConvergedSidecarFactoryOptions} for the Agent OS web-target
 * wasm kernel (with the ACP extension). Pass the result to
 * `createBrowserRuntimeDriverFactory`'s `convergedSidecar` option:
 *
 * ```ts
 * import { createBrowserRuntimeDriverFactory } from "@rivet-dev/agentos-runtime-browser";
 * import { createAgentOsConvergedSidecar } from "@rivet-dev/agentos-browser";
 *
 * const factory = createBrowserRuntimeDriverFactory({
 *   convergedSidecar: createAgentOsConvergedSidecar(config),
 * });
 * ```
 */
export function createAgentOsConvergedSidecar(
	config: CreateVmConfig,
	options: AgentOsConvergedSidecarOptions = {},
): ConvergedSidecarFactoryOptions {
	const moduleUrl = options.moduleUrl ?? WASM_MODULE_URL;
	const binaryUrl = options.binaryUrl ?? WASM_BINARY_URL;
	return {
		config,
		...(options.packageBytes === undefined
			? {}
			: {
					packages: options.packageBytes.map((content) => ({ content })),
				}),
		...(options.packagesMountAt === undefined
			? {}
			: { packagesMountAt: options.packagesMountAt }),
		codec: options.codec ?? "bare",
		onFsReadDenied: options.onFsReadDenied,
		async loadSidecar(): Promise<ConvergedSidecarHandle> {
			const host = createConvergedExecutionHostBridge({
				agentExecutor: options.agentExecutor,
			});
			const wasmModule = (await import(
				/* @vite-ignore */ String(moduleUrl)
			)) as AgentOsSidecarWasmWebModule;
			await wasmModule.default(String(binaryUrl));
			const sidecar = new wasmModule.AgentOsBrowserSidecarWasm(host.bridge);
			const rawPushFrame = (frame: Uint8Array): Uint8Array => {
				const response = sidecar.pushFrame(frame);
				if (!(response instanceof Uint8Array)) {
					throw new Error("agentos wasm sidecar returned no response frame");
				}
				return response;
			};
			const pushFrame = createAcpPendingResponseDriver({
				pushFrame: rawPushFrame,
				host,
				...(options.isAgentInteractionCancelled === undefined
					? {}
					: { isCancelled: options.isAgentInteractionCancelled }),
				frameHelpers: {
					pendingResponseProcessId(frame) {
						const processId = sidecar.pendingResponseProcessId(frame);
						if (processId === null) return null;
						if (typeof processId !== "string") {
							throw new Error(
								"agentos wasm sidecar returned an invalid pending process id",
							);
						}
						return processId;
					},
					pendingResponseTimeoutMs(frame) {
						const timeoutMs = sidecar.pendingResponseTimeoutMs(frame);
						if (timeoutMs === null) return null;
						if (
							typeof timeoutMs !== "number" ||
							!Number.isInteger(timeoutMs) ||
							timeoutMs <= 0
						) {
							throw new Error(
								"agentos wasm sidecar returned an invalid pending timeout",
							);
						}
						return timeoutMs;
					},
					pendingResponseTimeoutPhase(frame) {
						const phase = sidecar.pendingResponseTimeoutPhase(frame);
						if (phase === null) return null;
						if (typeof phase !== "string" || phase.length === 0) {
							throw new Error(
								"agentos wasm sidecar returned an invalid pending timeout phase",
							);
						}
						return phase;
					},
					buildDeliverAgentOutputFrame(originResponse, processId, chunk) {
						const frame = sidecar.buildDeliverAgentOutputFrame(
							originResponse,
							processId,
							chunk,
						);
						if (!(frame instanceof Uint8Array)) {
							throw new Error(
								"agentos wasm sidecar returned no ACP delivery frame",
							);
						}
						return frame;
					},
					buildDeliverAgentStderrFrame(originResponse, processId, chunk) {
						const frame = sidecar.buildDeliverAgentStderrFrame(
							originResponse,
							processId,
							chunk,
						);
						if (!(frame instanceof Uint8Array)) {
							throw new Error(
								"agentos wasm sidecar returned no ACP stderr-delivery frame",
							);
						}
						return frame;
					},
					buildAbortPendingFrame(originResponse, processId, reason, exitCode) {
						const frame = sidecar.buildAbortPendingFrame(
							originResponse,
							processId,
							reason,
							exitCode,
						);
						if (!(frame instanceof Uint8Array)) {
							throw new Error(
								"agentos wasm sidecar returned no ACP abort frame",
							);
						}
						return frame;
					},
					restorePendingResponse(originResponse, completedResponse) {
						const frame = sidecar.restorePendingResponse(
							originResponse,
							completedResponse,
						);
						if (!(frame instanceof Uint8Array)) {
							throw new Error(
								"agentos wasm sidecar returned no restored ACP response",
							);
						}
						return frame;
					},
				},
			});
			return {
				pushFrame,
				setNextExecutionId: (executionId: string) => {
					host.setNextExecutionId(executionId);
				},
			};
		},
	};
}
