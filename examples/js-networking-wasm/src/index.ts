import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

// The runtime exposes its underlying VM for advanced shell and WASM workflows.
const runtime = await JavaScriptRuntime.create({
	permissions: { network: "allow" },
});
try {
	const result = await runtime.vm.process.execFile("sh", [
		"-c",
		"printf 'hello from a WASM-backed AgentOS command\\n'",
	]);
	console.log(result.stdout.trim());
} finally {
	await runtime.dispose();
}
