import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";

const source = readFileSync(
	new URL(
		"../../../crates/executor-v8-runtime/assets/runners/wasm-runner.mjs",
		import.meta.url,
	),
	"utf8",
);
const stdinSource = source.slice(
	source.indexOf("function readKernelStdinChunk("),
	source.indexOf("const rawModuleBytes ="),
);
const ttyStart = source.indexOf("const hostTtyImport = {");
const ttyReadSource = source.slice(
	source.indexOf("  read(ptr, len, timeoutMs) {", ttyStart),
	source.indexOf("  // `host_tty.isatty", ttyStart),
);

// Execute the production adapter with a bounded direct-reply fixture. The
// fixture supplies one interrupted syscall and one byte, with no VM or I/O.
function fixture(
	kind: "stdin" | "tty",
	restart: boolean,
	code = "EINTR",
	timeoutMs = 0xffffffff,
	onInterrupt?: () => void,
) {
	const events: string[] = [];
	const error = Object.assign(new Error("interrupted fixture"), { code });
	const memory = new ArrayBuffer(16);
	let reads = 0;
	const callSyncRpc = () => {
		events.push("read");
		if (++reads === 1) {
			onInterrupt?.();
			throw error;
		}
		if (reads > 2) throw new Error("unexpected extra read");
		return { dataBase64: Buffer.from("x").toString("base64") };
	};
	const dispatch = (restartable: boolean) => {
		expect(restartable).toBe(true);
		events.push("handler");
		return !restart;
	};
	const body =
		kind === "stdin"
			? `${stdinSource}; return () => readKernelStdinChunk(1);`
			: `const tty = { ${ttyReadSource} }; return () => tty.read(0, 1, ${timeoutMs});`;
	const run = new Function(
		"callSyncRpc",
		"dispatchPendingWasmSignals",
		"Buffer",
		"KERNEL_WAIT_SLICE_MS",
		"KERNEL_STDIN_WOULD_BLOCK",
		"instanceMemory",
		"guestRangeIsValid",
		body,
	)(
		callSyncRpc,
		dispatch,
		Buffer,
		10_000,
		Symbol(),
		{ buffer: memory },
		() => true,
	);
	return { run, events, error, memory };
}

describe("WASM interrupted stdin adapters", () => {
	it("the WASI stdin adapter preserves the interrupted read errno", () => {
		const error = Object.assign(new Error("caught signal"), { code: "EINTR" });
		let mapped: unknown;
		const dependencies = {
			validateGuestIovRequest: () => ({ errno: 0, totalLength: 1 }),
			WASI_ERRNO_SUCCESS: 0,
			WASI_ERRNO_FAULT: 21,
			guestRangeIsValid: () => true,
			getHostNetSocket: () => null,
			__agentOSWasiMeasurePhase: (
				_name: string,
				_phase: string,
				run: () => unknown,
			) => run(),
			lookupFdHandle: () => ({ kind: "passthrough", targetFd: 0 }),
			SIDECAR_MANAGED_PROCESS: true,
			KERNEL_STDIO_SYNC_RPC: true,
			delegatedWasiFdFlags: new Map(),
			WASI_FDFLAGS_NONBLOCK: 4,
			readKernelStdinChunk: () => {
				throw error;
			},
			mapHostProcessError: (caught: unknown) => {
				mapped = caught;
				return 27;
			},
		};
		const fdReadSource = source.slice(
			source.indexOf("wasiImport.fd_read = ("),
			source.indexOf("wasiImport.fd_readdir = ("),
		);
		const run = new Function(
			...Object.keys(dependencies),
			`const wasiImport = {}; ${fdReadSource}; return () => wasiImport.fd_read(0, 0, 1, 8);`,
		)(...Object.values(dependencies));
		expect(run()).toBe(27);
		expect(mapped).toBe(error);
	});

	it.each([
		"stdin",
		"tty",
	] as const)("%s dispatches a restarting handler before reissuing read", (kind) => {
		const control = fixture(kind, true);
		const result = control.run();
		expect(control.events).toEqual(["read", "handler", "read"]);
		if (kind === "stdin") expect(result).toEqual(Buffer.from("x"));
		else {
			expect(result).toBe(1);
			expect(new Uint8Array(control.memory)[0]).toBe(120);
		}
	});

	it("non-restarting stdin preserves EINTR after dispatching the handler", () => {
		const control = fixture("stdin", false);
		expect(control.run).toThrow(control.error);
		expect(control.events).toEqual(["read", "handler"]);
	});

	it("terminal event polling retries EINTR even without SA_RESTART, like crossterm", () => {
		const control = fixture("tty", false);
		expect(control.run()).toBe(1);
		expect(control.events).toEqual(["read", "handler", "read"]);
	});

	it("terminal EINTR does not reset the original finite deadline", () => {
		vi.useFakeTimers();
		try {
			vi.setSystemTime(0);
			const control = fixture("tty", true, "EINTR", 10, () =>
				vi.setSystemTime(20),
			);
			expect(control.run()).toBe(0);
			expect(control.events).toEqual(["read", "handler"]);
		} finally {
			vi.useRealTimers();
		}
	});

	it.each([
		"stdin",
		"tty",
	] as const)("%s preserves unrelated failures", (kind) => {
		const control = fixture(kind, true, "EIO");
		expect(control.run).toThrow(control.error);
		expect(control.events).toEqual(["read"]);
	});
});
