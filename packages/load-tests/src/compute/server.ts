// Rivet Compute load-runner application.
//
// One AgentOS actor, `agentosLoadRunner`, is deployed to Rivet Compute. The
// external controller (`compute-load`) drives keyed instances of this actor
// from outside Compute so target saturation cannot hide the offered load.
//
// Design constraints (see docs-internal/load-testing.md, checklist J):
//   - Keep `registry.start()`; do not hand-mount a router. RivetKit listens on
//     RIVET_PORT (default 3000) and serves `/api/rivet/metadata`.
//   - Every actor gets strict per-actor AgentOS process/memory/output/time
//     limits so one Compute instance cannot exhaust its container.
//   - The bounded AgentOS work surface is the built-in `execArgv` / `spawn`
//     action set; its bounds are the `limits` object below. The controller
//     only calls these bounded actions.
//   - Live VM handles are process-local. The actor persists nothing but the
//     RivetKit-managed VM/session state; on migration the VM is recreated
//     lazily by `ensureVm` on the next action, and any in-flight in-memory
//     guest work is dropped (reported as interrupted by the controller, which
//     sees the failed action), never silently duplicated.
import { agentOS, setup } from "@rivet-dev/agentos";

// Strict per-actor envelope. These bound a single Compute instance's AgentOS
// footprint; the controller varies actor COUNT and work-per-actor on top.
const agentosLoadRunner = agentOS({
	// No default software: the load workloads only need node/exec, keeping
	// per-instance memory low so actor count, not package weight, drives scale.
	defaultSoftware: false,
	limits: {
		resources: {
			maxProcesses: 32,
			maxOpenFds: 128,
			maxSockets: 64,
			maxFilesystemBytes: 64 * 1024 * 1024,
			maxWasmFuel: 30_000,
			maxWasmMemoryBytes: 64 * 1024 * 1024,
			maxWasmStackBytes: 2 * 1024 * 1024,
		},
		process: {
			pendingStdinBytes: 4 * 1024 * 1024,
			pendingEventCount: 4_000,
			pendingEventBytes: 16 * 1024 * 1024,
		},
		jsRuntime: {
			v8HeapLimitMb: 96,
			cpuTimeLimitMs: 15_000,
			wallClockLimitMs: 20_000,
			importCacheMaterializeTimeoutMs: 20_000,
			syncRpcWaitTimeoutMs: 20_000,
			capturedOutputLimitBytes: 8 * 1024 * 1024,
		},
		python: {
			executionTimeoutMs: 30_000,
			maxOldSpaceMb: 0,
		},
		wasm: {
			prewarmTimeoutMs: 20_000,
			runnerHeapLimitMb: 512,
		},
	},
});

export const registry = setup({ use: { agentosLoadRunner } });

registry.start();
