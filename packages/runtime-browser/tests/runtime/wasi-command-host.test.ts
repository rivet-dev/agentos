import { posix } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { POLYFILL_CODE_MAP } from "../../src/runtime.js";

type WasiCommandHost = {
	setParentWasi(wasi: unknown): WasiCommandHost;
	setMemory(memory: WebAssembly.Memory): WasiCommandHost;
	installBlockingStdin(processLike: unknown): WasiCommandHost;
	imports: {
		host_fs: { fd_mode(fd: number): number };
		host_process: {
			fd_dup2(oldFd: number, newFd: number): number;
			proc_spawn(
				argvPtr: number,
				argvLen: number,
				envpPtr: number,
				envpLen: number,
				stdinFd: number,
				stdoutFd: number,
				stderrFd: number,
				cwdPtr: number,
				cwdLen: number,
				retPid: number,
			): number;
		};
	};
};

const wasiGlobal = globalThis as typeof globalThis & {
	__agentOSWasiHost?: { lookupFdHandle?: (fd: number) => unknown };
};
const originalWasiHost = wasiGlobal.__agentOSWasiHost;

afterEach(() => {
	if (originalWasiHost === undefined) delete wasiGlobal.__agentOSWasiHost;
	else wasiGlobal.__agentOSWasiHost = originalWasiHost;
});

describe("browser WASI command host", () => {
	it("keeps WASI-local descriptors out of the synthetic child descriptor table", async () => {
		const source = POLYFILL_CODE_MAP["secure-exec:wasi-command-host"];
		const module = { exports: {} as Record<string, unknown> };
		const mockFs = {
			fstatSync: () => ({ mode: 0o100640, size: 7 }),
		};
		const mockPath = { posix };
		new Function("module", "exports", "require", source)(
			module,
			module.exports,
			(name: string) => (name === "node:fs" ? mockFs : mockPath),
		);
		const createWasiCommandHost = module.exports.createWasiCommandHost as (
			options: Record<string, unknown>,
		) => Promise<WasiCommandHost>;

		wasiGlobal.__agentOSWasiHost = {};
		const host = await createWasiCommandHost({ commands: {} });
		host.setParentWasi({
			fdTable: new Map([
				[
					4,
					{
						kind: "file",
						realFd: 73,
						offset: 0,
						readOnly: false,
					},
				],
			]),
		});
		host.installBlockingStdin({
			stdin: { read: () => null, readableLength: 0 },
			stdout: { write: () => undefined },
			stderr: { write: () => undefined },
		});

		// Local fd 4 remains owned by the active WASI instance. This is the
		// distinction that lets WASI close/delete its own table entry.
		expect(wasiGlobal.__agentOSWasiHost.lookupFdHandle?.(4)).toBeNull();
		expect(host.imports.host_fs.fd_mode(4)).toBe(0o100640);

		// Explicit dup2 is process glue, so it creates an isolated synthetic
		// descriptor without exposing the source fd as an external handle.
		expect(host.imports.host_process.fd_dup2(4, 1)).toBe(0);
		expect(wasiGlobal.__agentOSWasiHost.lookupFdHandle?.(1)).toMatchObject({
			kind: "guest-file",
			targetFd: 73,
		});
		expect(wasiGlobal.__agentOSWasiHost.lookupFdHandle?.(4)).toBeNull();
	});

	it("dispatches executable shebang scripts through a guest WASM interpreter", async () => {
		const source = POLYFILL_CODE_MAP["secure-exec:wasi-command-host"];
		const module = { exports: {} as Record<string, unknown> };
		const script = new TextEncoder().encode(
			"#!/bin/sh\necho BROWSER_SCRIPT_OK\n",
		);
		new Function("module", "exports", "require", source)(
			module,
			module.exports,
			(name: string) => {
				if (name === "node:fs") {
					return {
						readFileSync: (path: string) =>
							path === "/tmp/proof.sh" ? script : null,
					};
				}
				return { posix };
			},
		);
		const createWasiCommandHost = module.exports.createWasiCommandHost as (
			options: Record<string, unknown>,
		) => Promise<WasiCommandHost>;
		const interpreter = new WebAssembly.Module(
			Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0),
		);
		let childArgs: string[] | undefined;
		class FakeWasi {
			readonly args: string[];
			readonly fdTable = new Map();
			readonly wasiImport = {};

			constructor(options: { args: string[] }) {
				this.args = options.args;
			}

			start() {
				childArgs = this.args;
				return 0;
			}
		}

		wasiGlobal.__agentOSWasiHost = {};
		const host = await createWasiCommandHost({
			WASI: FakeWasi,
			commands: { sh: interpreter },
			cwd: "/",
		});
		const memory = new WebAssembly.Memory({ initial: 1 });
		const argv = new TextEncoder().encode("/tmp/proof.sh\0argument\0");
		new Uint8Array(memory.buffer).set(argv, 32);
		host.setParentWasi({ fdTable: new Map() });
		host.setMemory(memory);
		host.installBlockingStdin({
			stdin: { read: () => null, readableLength: 0 },
			stdout: { write: () => undefined },
			stderr: { write: () => undefined },
		});

		expect(
			host.imports.host_process.proc_spawn(
				32,
				argv.length,
				0,
				0,
				0,
				1,
				2,
				0,
				0,
				256,
			),
		).toBe(0);
		expect(childArgs).toEqual(["/bin/sh", "/tmp/proof.sh", "argument"]);
		expect(new DataView(memory.buffer).getUint32(256, true)).toBe(100);
	});
});
