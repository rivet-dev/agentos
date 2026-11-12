import { readFileSync } from "node:fs";
import { describe, expect, test, vi } from "vitest";

const source = readFileSync(
	new URL(
		"../../../crates/executor-v8-runtime/assets/runners/wasm-runner.mjs",
		import.meta.url,
	),
	"utf8",
);
const helper = source.slice(
	source.indexOf("function kernelFdForHandle("),
	source.indexOf("function kernelFdMappingsForSpawn("),
);
const filestat = source.slice(
	source.indexOf("wasiImport.fd_filestat_get = (fd, statPtr) => {"),
	source.indexOf("wasiImport.fd_filestat_set_size = (fd, size) => {"),
);

const setFlags = source.slice(
	source.indexOf("wasiImport.fd_fdstat_set_flags = (fd, flags) => {"),
	source.indexOf("wasiImport.fd_filestat_get = (fd, statPtr) => {"),
);

function fixture(target: number | null, managed = true, validMemory = true) {
	const stat = { filetype: 0, mode: 0o010600, size: 0, uid: 1234, gid: 2345 };
	const rpc = vi.fn(() => stat);
	const delegate = vi.fn(() => 8);
	const write = vi.fn(() => 0);
	const dependencies = {
		SIDECAR_MANAGED_PROCESS: managed,
		KERNEL_STDIO_SYNC_RPC: false,
		lookupFdHandle: () =>
			target === null ? null : { kind: "passthrough", targetFd: target },
		guestRangeIsValid: () => validMemory,
		callSyncRpc: rpc,
		writeGuestFilestat: write,
		delegateManagedFdFilestatGet: delegate,
		rejectClosedPassthroughFd: () => target === null,
		WASI_ERRNO_BADF: 8,
		WASI_ERRNO_FAULT: 21,
		mapHostProcessError: () => 8,
		getHostNetSocket: () => null,
		WASI_ERRNO_SUCCESS: 0,
		WASI_FDFLAGS_APPEND: 1,
		WASI_FDFLAGS_NONBLOCK: 4,
		WASI_FDFLAGS_AGENTOS_DIRECT: 32,
		KERNEL_O_APPEND: 1024,
		KERNEL_O_NONBLOCK: 2048,
		KERNEL_O_DIRECT: 16384,
		delegateManagedFdFdstatSetFlags: delegate,
		delegatedWasiFdFlags: new Map(),
	};
	const run = new Function(
		...Object.keys(dependencies),
		`${helper}; const wasiImport = {}; ${filestat}; ${setFlags}; return wasiImport;`,
	)(...Object.values(dependencies)) as {
		fd_filestat_get: (fd: number, ptr: number) => number;
		fd_fdstat_set_flags: (fd: number, flags: number) => number;
	};
	return {
		run: run.fd_filestat_get,
		setFlags: run.fd_fdstat_set_flags,
		rpc,
		delegate,
		write,
		stat,
	};
}

describe("WASM managed stdio metadata routing", () => {
	test("F_SETFL updates the same guest kernel descriptor used by F_GETFL", () => {
		const f = fixture(0);
		expect(f.setFlags(0, 4)).toBe(0);
		expect(f.rpc).toHaveBeenCalledWith("process.fd_set_flags", [0, 2048]);
		expect(f.delegate).not.toHaveBeenCalled();
	});

	for (const fd of [0, 1, 2])
		test(`queries guest kernel fd${fd} without touching bootstrap metadata`, () => {
			const f = fixture(fd);
			expect(f.run(fd, 16)).toBe(0);
			expect(f.rpc).toHaveBeenCalledTimes(1);
			expect(f.rpc).toHaveBeenCalledWith("process.fd_filestat", [fd]);
			expect(f.write).toHaveBeenCalledTimes(1);
			expect(f.write).toHaveBeenCalledWith(16, f.stat, f.stat.filetype);
			expect(f.delegate).not.toHaveBeenCalled();
		});
	test("stdio aliases retain the target descriptor identity", () => {
		const f = fixture(0);
		expect(f.run(17, 16)).toBe(0);
		expect(f.rpc).toHaveBeenCalledTimes(1);
		expect(f.rpc).toHaveBeenCalledWith("process.fd_filestat", [0]);
	});
	test("closed stdio cannot expose a private bootstrap descriptor", () => {
		const f = fixture(null);
		expect(f.run(0, 16)).toBe(8);
		expect(f.setFlags(0, 4)).toBe(8);
		expect(f.rpc).not.toHaveBeenCalled();
		expect(f.delegate).not.toHaveBeenCalled();
	});
	test("guest memory is checked before a kernel metadata query", () => {
		const f = fixture(0, true, false);
		expect(f.run(0, 16)).toBe(21);
		expect(f.rpc).not.toHaveBeenCalled();
	});
	test("standalone passthrough retains its existing WASI delegate", () => {
		const f = fixture(0, false);
		expect(f.run(0, 16)).toBe(8);
		expect(f.delegate).toHaveBeenCalledTimes(1);
		expect(f.delegate).toHaveBeenCalledWith(0, 16);
		expect(f.setFlags(0, 4)).toBe(8);
		expect(f.delegate).toHaveBeenCalledWith(0, 4);
		expect(f.rpc).not.toHaveBeenCalled();
	});
});
