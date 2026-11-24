import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Runs the actual release binary on every supported OS/CPU. Cross-compilation
// alone cannot detect unsupported thread-stack discovery or snapshot startup.

// A WASM command that writes "native-wasm\n" to stdout through fd_write.
const WASM_HELLO = Uint8Array.from([
	0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0c, 0x02, 0x60, 0x04,
	0x7f, 0x7f, 0x7f, 0x7f, 0x01, 0x7f, 0x60, 0x00, 0x00, 0x02, 0x23, 0x01, 0x16,
	0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61, 0x70, 0x73, 0x68, 0x6f, 0x74,
	0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65, 0x77, 0x31, 0x08, 0x66, 0x64, 0x5f,
	0x77, 0x72, 0x69, 0x74, 0x65, 0x00, 0x00, 0x03, 0x02, 0x01, 0x01, 0x05, 0x03,
	0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79,
	0x02, 0x00, 0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01, 0x0a, 0x1d,
	0x01, 0x1b, 0x00, 0x41, 0x00, 0x41, 0x10, 0x36, 0x02, 0x00, 0x41, 0x04, 0x41,
	0x0c, 0x36, 0x02, 0x00, 0x41, 0x01, 0x41, 0x00, 0x41, 0x01, 0x41, 0x08, 0x10,
	0x00, 0x1a, 0x0b, 0x0b, 0x12, 0x01, 0x00, 0x41, 0x10, 0x0b, 0x0c, 0x6e, 0x61,
	0x74, 0x69, 0x76, 0x65, 0x2d, 0x77, 0x61, 0x73, 0x6d, 0x0a,
]);

// A WASM command that recurses until it exhausts the guest stack.
const WASM_RECURSE = Uint8Array.from([
	0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00,
	0x00, 0x03, 0x03, 0x02, 0x00, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13,
	0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x5f, 0x73,
	0x74, 0x61, 0x72, 0x74, 0x00, 0x01, 0x0a, 0x0b, 0x02, 0x04, 0x00, 0x10, 0x00,
	0x0b, 0x04, 0x00, 0x10, 0x00, 0x0b,
]);
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
				// WASM commands run on native V8 isolates with their own threads and
				// stack limits, so exercise them on every platform too.
				await vm.writeFile("/tmp/wasm-hello", WASM_HELLO);
				await vm.writeFile("/tmp/wasm-recurse", WASM_RECURSE);
				const mode = await vm.process.execFile(
					"node",
					[
						"-e",
						"for (const f of ['/tmp/wasm-hello', '/tmp/wasm-recurse']) require('node:fs').chmodSync(f, 0o755)",
					],
					{ timeoutMs: 10_000, output: { capture: "all" } },
				);
				expect(mode.exitCode, mode.stderr).toBe(0);
				const hello = await vm.process.execFile("/tmp/wasm-hello", [], {
					timeoutMs: 10_000,
					output: { capture: "all" },
				});
				expect(hello.exitCode, hello.stderr).toBe(0);
				expect(hello.stdout).toBe("native-wasm\n");
				const overflow = await vm.process.execFile("/tmp/wasm-recurse", [], {
					timeoutMs: 10_000,
					output: { capture: "all" },
				});
				expect(overflow.exitCode).not.toBe(0);
				expect(overflow.stderr).toContain("ERR_AGENTOS_WASM_STACK_EXHAUSTED");
				const again = await vm.process.execFile("/tmp/wasm-hello", [], {
					timeoutMs: 10_000,
					output: { capture: "all" },
				});
				expect(again.stdout).toBe("native-wasm\n");
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
