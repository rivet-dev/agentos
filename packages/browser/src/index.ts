// @rivet-dev/agentos-browser — converged browser runtime for Agent OS.
//
// The browser runtime is @secure-exec/browser's CONVERGED stack (worker,
// SharedArrayBuffer sync-bridge, fs/net/dns/module servicers, all enforced by the
// wasm kernel). Agent OS does not carry its own copy; it re-exports that runtime
// and adds only the ACP/wasm-sidecar layer (createAgentOsConvergedSidecar). The
// pre-convergence TS-kernel files (worker/runtime/driver/sync-bridge/permission
// eval) were deleted in the reconciliation — the kernel is the sole enforcement
// point, so guest-side permission eval no longer exists here.
//
// Per the converged model (kernel-owns-fs), per-runtime OPFS *namespace* helpers
// (listOpfsNamespaces/releaseOpfsNamespace) are gone: storage isolation is the
// kernel's responsibility, not a TS-layer concern.

// --- Converged runtime, re-exported from @secure-exec/browser ---
export type {
	BrowserDriverOptions,
	BrowserRuntimeSystemOptions,
} from "@secure-exec/browser";
export {
	createBrowserDriver,
	createBrowserNetworkAdapter,
	createOpfsFileSystem,
	InMemoryFileSystem,
} from "@secure-exec/browser";
export type {
	ExecOptions,
	ExecResult,
	NodeRuntimeDriver,
	StdioChannel,
	StdioEvent,
	TimingMitigation,
} from "@secure-exec/browser";
export {
	allowAll,
	allowAllChildProcess,
	allowAllEnv,
	allowAllFs,
	allowAllNetwork,
	createInMemoryFileSystem,
} from "@secure-exec/browser";
export type {
	BrowserRuntimeDriverFactoryOptions,
	ConvergedSidecarFactoryOptions,
	ConvergedSidecarHandle,
} from "@secure-exec/browser";
export { createBrowserRuntimeDriverFactory } from "@secure-exec/browser";
export type { WorkerHandle } from "@secure-exec/browser";
export { BrowserWorkerAdapter } from "@secure-exec/browser";

// --- Agent OS converged layer: plug the ACP wasm sidecar into the runtime ---
export type { AgentOsConvergedSidecarOptions } from "./converged-sidecar.js";
export { createAgentOsConvergedSidecar } from "./converged-sidecar.js";
export type { ConvergedExecutionHostBridge } from "./converged-execution-host-bridge.js";
export { createConvergedExecutionHostBridge } from "./converged-execution-host-bridge.js";
