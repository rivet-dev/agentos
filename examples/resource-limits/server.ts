import { createAgentOsClient } from "@rivet-dev/agentos";

const client = createAgentOsClient();
const vm = client.agentOS.getOrCreate(["examples", "resource-limits"], {
	createWithInput: {
		config: {
			limits: {
				resources: {
					maxProcesses: 64, // concurrent processes
					maxOpenFds: 256, // open file descriptors
					maxSockets: 128, // open sockets
					maxFilesystemBytes: 256 * 1024 * 1024, // VFS storage budget
					maxWasmMemoryBytes: 128 * 1024 * 1024, // WASM linear memory
					maxWasmStackBytes: 4 * 1024 * 1024, // WASM call-stack ceiling
				},
				process: {
					pendingStdinBytes: 64 * 1024 * 1024, // stdin waiting on a kernel pipe
					pendingEventCount: 10_000, // queued process/runtime events per stage
					pendingEventBytes: 64 * 1024 * 1024, // queued event payload per stage
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
					activeCpuTimeLimitMs: 30_000, // active WASM CPU time
					wallClockLimitMs: 30_000, // elapsed WASM execution time
					prewarmTimeoutMs: 30_000, // WASM compile-cache warmup
					runnerHeapLimitMb: 2048, // trusted WASI runner V8 heap
				},
			},
		},
	},
});

console.log(await vm.vm.status({}));
