import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Runs the actual release binary on every supported OS/CPU. Cross-compilation
// alone cannot detect unsupported thread-stack discovery or snapshot startup.
test("native release creates, interrupts and disposes VMs on a reused sidecar", async () => {
	const sidecar = await AgentOs.createSidecar();
	try {
		for (let iteration = 0; iteration < 3; iteration++) {
			const vm = await AgentOs.create({
				sidecar: { kind: "explicit", handle: sidecar },
				defaultSoftware: false,
				permissions: { network: "deny" },
			});
			try {
				const result = await vm.javascript.evaluate("21 * 2");
				expect(result.outcome).toBe("succeeded");
				expect(result.value).toBe(42);
				await vm.filesystem.writeFile("/home/agentos/platform.txt", "native\n");
				expect(
					new TextDecoder().decode(
						await vm.filesystem.readFile("/home/agentos/platform.txt"),
					),
				).toBe("native\n");
				const wasm = await vm.javascript.evaluate(`(() => {
					const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,3,2,1,0,7,10,1,6,97,110,115,119,101,114,0,0,10,6,1,4,0,65,42,11]);
					return new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports.answer()
				})()`);
				expect(wasm.outcome, JSON.stringify(wasm)).toBe("succeeded");
				expect(wasm.value).toBe(42);
				const interrupted = await vm.javascript.evaluate(
					"(() => { for (;;) {} })()",
					{
						timeoutMs: 250,
					},
				);
				expect(interrupted.outcome).toBe("timed_out");
				const recovered = await vm.javascript.evaluate("6 * 7");
				expect(recovered.outcome).toBe("succeeded");
				expect(recovered.value).toBe(42);
			} finally {
				await vm.dispose();
			}
		}
	} finally {
		await sidecar.dispose();
	}
}, 60_000);
