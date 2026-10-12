import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source = readFileSync(
	new URL("../../build-tools/bridge-src/builtins/console.ts", import.meta.url),
	"utf8",
);
const adapterSource = source.slice(
	source.indexOf("function _getStdoutIsTTY("),
	source.indexOf("function formatConsoleValue("),
);
function adapter(write: (bytes: Uint8Array | string) => number | null) {
	const tasks: Array<() => void> = [];
	const streams = new Function(
		"import_buffer2",
		"_queueMicrotask",
		"_resolveRuntimeTtyConfig",
		"_log",
		"_error",
		"_kernelStdioWriteRaw",
		`${adapterSource}\nreturn {stdout: _stdout, stderr: _stderr};`,
	)(
		{ Buffer },
		(task: () => void) => tasks.push(task),
		() => ({ stdoutIsTTY: false, stderrIsTTY: false }),
		{ applySync: (_: unknown, args: [Uint8Array | string]) => write(args[0]) },
		{ applySync: (_: unknown, args: [Uint8Array | string]) => write(args[0]) },
		{},
	);
	return {
		...streams,
		flush: () => {
			while (tasks.length) tasks.shift()!();
		},
	};
}

describe("Node stdio write completion", () => {
	it("returns failure and delivers callback, error, then close without synchronous throw", () => {
		const failure = Object.assign(new Error("bad descriptor"), {
			code: "EBADF",
		});
		const stream = adapter(() => {
			throw failure;
		});
		const events: string[] = [];
		stream.stdout.on("error", (error: Error) => {
			expect(error).toBe(failure);
			events.push("error");
		});
		stream.stdout.on("close", () => events.push("close"));
		expect(
			stream.stdout.write("lost", (error: Error) => {
				expect(error).toBe(failure);
				events.push("callback");
			}),
		).toBe(false);
		expect(events).toEqual([]);
		stream.flush();
		expect(events).toEqual(["callback", "error", "close"]);
	});
	it("completes partial writes with bounded binary frames", () => {
		const parts: Buffer[] = [];
		const input = Buffer.alloc(130_007, 0xa3);
		const stream = adapter((bytes) => {
			const chunk =
				typeof bytes === "string" ? Buffer.from(bytes) : Buffer.from(bytes);
			expect(chunk.length).toBeLessThanOrEqual(64 * 1024);
			const count = Math.min(chunk.length, 8191);
			parts.push(chunk.subarray(0, count));
			return count;
		});
		expect(stream.stdout.write(input)).toBe(true);
		expect(Buffer.concat(parts)).toEqual(input);
	});
	it("preserves standalone null acknowledgement and explicit encoding", () => {
		const parts: Buffer[] = [];
		const stream = adapter((bytes) => {
			parts.push(
				typeof bytes === "string" ? Buffer.from(bytes) : Buffer.from(bytes),
			);
			return null;
		});
		expect(stream.stdout.write("c3a9", "hex")).toBe(true);
		expect(Buffer.concat(parts)).toEqual(Buffer.from("é"));
	});
	it("does not silently drop an unhandled write error", () => {
		const failure = Object.assign(new Error("bad descriptor"), {
			code: "EBADF",
		});
		const stream = adapter(() => {
			throw failure;
		});
		expect(stream.stdout.write("lost")).toBe(false);
		expect(() => stream.flush()).toThrow(failure);
	});
});
