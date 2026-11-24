import { PassThrough } from "node:stream";
import { describe, expect, test, vi } from "vitest";
import {
	SidecarProcessError,
	SidecarProcessExited,
	StdioSidecarProcess,
} from "../src/process.js";

describe("stdio sidecar process monitor", () => {
	test("formats process exit errors with stderr", () => {
		const error = new SidecarProcessExited({
			exitCode: 7,
			signal: null,
			stderr: "boom",
		});

		expect(error.message).toBe(
			"sidecar process exited with code 7\nstderr:\nboom",
		);
		expect(error.exitCode).toBe(7);
		expect(error.signal).toBeNull();
		expect(error.stderr).toBe("boom");
	});

	test("formats process spawn errors with stderr", () => {
		const error = new SidecarProcessError(new Error("missing"), "nope");

		expect(error.message).toBe("sidecar process error: missing\nstderr:\nnope");
		expect(error.childError.message).toBe("missing");
		expect(error.stderr).toBe("nope");
	});

	test("captures stderr chunks from a child-like process", async () => {
		const stderrWrite = vi
			.spyOn(process.stderr, "write")
			.mockImplementation(() => true);
		const child = createChildLike();
		const monitor = StdioSidecarProcess.fromChild(child);

		child.stderr.write("first");
		child.stderr.write(Buffer.from(" second"));

		expect(monitor.stderrText()).toBe("first second");
		stderrWrite.mockRestore();
	});

	test("forwards live stderr and retains only a bounded excerpt", () => {
		const forwarded: Buffer[] = [];
		const stderrWrite = vi
			.spyOn(process.stderr, "write")
			.mockImplementation((chunk: unknown) => {
				forwarded.push(Buffer.from(chunk as Uint8Array));
				return true;
			});
		try {
			const child = createChildLike();
			const monitor = StdioSidecarProcess.fromChild(child);

			// 1 MiB of guest-driven sidecar diagnostics, in many small chunks.
			const chunkSize = 1024;
			const chunkCount = 1024;
			for (let index = 0; index < chunkCount; index += 1) {
				const label = `[chunk ${String(index).padStart(4, "0")}]`;
				child.stderr.write(label + "x".repeat(chunkSize - label.length));
			}

			const retained = monitor.stderrText();
			expect(Buffer.byteLength(retained)).toBeLessThanOrEqual(64 * 1024 + 256);
			expect(retained.startsWith("[chunk 0000]")).toBe(true);
			expect(retained).toContain(`[chunk ${chunkCount - 1}]`);
			expect(retained).not.toContain("[chunk 0512]");
			expect(retained).toContain(
				`${chunkCount * chunkSize - 64 * 1024} bytes of sidecar stderr dropped`,
			);

			const forwardedText = Buffer.concat(forwarded).toString("utf8");
			expect(forwardedText.length).toBe(chunkCount * chunkSize);
			expect(forwardedText).toContain("[chunk 0512]");
		} finally {
			stderrWrite.mockRestore();
		}
	});
});

function createChildLike(): Parameters<
	typeof StdioSidecarProcess.fromChild
>[0] {
	return {
		stdin: new PassThrough(),
		stdout: new PassThrough(),
		stderr: new PassThrough(),
		stdio: [null, null, null, new PassThrough()],
		exitCode: null,
		signalCode: null,
		on() {
			return this;
		},
		off() {
			return this;
		},
	} as unknown as Parameters<typeof StdioSidecarProcess.fromChild>[0];
}
