import pi from "@agentos-software/pi";
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	software: [pi],
	limits: {
		resources: {
			maxProcesses: 64, // concurrent processes
			maxOpenFds: 256, // open file descriptors
			maxSockets: 128, // open sockets
			maxFilesystemBytes: 256 * 1024 * 1024, // VFS storage budget
			maxWasmFuel: 30_000, // WASM execution budget
			maxWasmMemoryBytes: 128 * 1024 * 1024, // WASM linear memory
			maxWasmStackBytes: 4 * 1024 * 1024, // WASM call-stack ceiling
		},
		jsRuntime: {
			v8HeapLimitMb: 128, // JS isolate heap
			cpuTimeLimitMs: 30_000, // active JS CPU time
			wallClockLimitMs: 0, // 0 disables elapsed wall-clock cutoff
			importCacheMaterializeTimeoutMs: 30_000, // Node import-cache setup
			syncRpcWaitTimeoutMs: 30_000, // host sync-RPC wait
		},
		python: {
			executionTimeoutMs: 300_000, // Python wall-clock execution
			maxOldSpaceMb: 0, // 0 keeps the Pyodide runner default
		},
		wasm: {
			prewarmTimeoutMs: 30_000, // WASM compile-cache warmup
			runnerHeapLimitMb: 2048, // trusted WASI runner V8 heap
		},
	},
});

export const registry = setup({ use: { vm } });
registry.start();
