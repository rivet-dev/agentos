import { AgentOs } from "@rivet-dev/agentos-core";

// Select Wasmtime for standalone WASM commands. The sidecar must include
// the wasm-wasmtime feature. JavaScript continues to use V8.
const vm = await AgentOs.create({ wasmBackend: "wasmtime" });

try {
	const result = await vm.process.exec("echo hello", {
		output: { capture: "all" },
	});
	console.log(result.stdout);
} finally {
	await vm.dispose();
}
