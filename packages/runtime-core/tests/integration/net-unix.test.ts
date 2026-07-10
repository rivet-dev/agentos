/**
 * Integration test for WasmVM Unix domain sockets.
 *
 * Spawns the unix_socket C program as WASM and connects to it from a guest Node
 * process through an AF_UNIX path.
 */

import { afterEach, describe, expect, it } from "vitest";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import {
	C_BUILD_DIR,
	COMMANDS_DIR,
	createIntegrationKernel,
	describeIf,
	skipUnlessWasmBuilt,
} from "@rivet-dev/agentos-vm-test-harness";
import type {
	IntegrationKernelResult,
	Kernel,
} from "@rivet-dev/agentos-vm-test-harness";

const WASM_UNIX_SOCKET = resolve(C_BUILD_DIR, "unix_socket");

function skipReason(): string | false {
	const wasmSkipReason = skipUnlessWasmBuilt();
	if (wasmSkipReason) return wasmSkipReason;
	if (!existsSync(WASM_UNIX_SOCKET)) {
		return `unix_socket WASM binary not found at ${WASM_UNIX_SOCKET}`;
	}
	return false;
}

interface RunningGuestProgram {
	process: ReturnType<Kernel["spawn"]>;
	stdoutChunks: Uint8Array[];
	stderrChunks: Uint8Array[];
}

function decodeChunks(chunks: Uint8Array[]): string {
	return chunks.map((chunk) => new TextDecoder().decode(chunk)).join("");
}

function spawnGuestProgram(
	kernel: Kernel,
	command: string,
	args: string[],
): RunningGuestProgram {
	const stdoutChunks: Uint8Array[] = [];
	const stderrChunks: Uint8Array[] = [];
	const process = kernel.spawn(command, args, {
		onStdout: (chunk) => stdoutChunks.push(chunk),
		onStderr: (chunk) => stderrChunks.push(chunk),
	});
	return { process, stdoutChunks, stderrChunks };
}

async function runGuestNodeProgram(
	kernel: Kernel,
	code: string,
): Promise<{ exitCode: number; stdout: string; stderr: string }> {
	const program = spawnGuestProgram(kernel, "node", ["-e", code]);
	const exitCode = await program.process.wait();
	return {
		exitCode,
		stdout: decodeChunks(program.stdoutChunks),
		stderr: decodeChunks(program.stderrChunks),
	};
}

async function waitForUnixListener(
	kernel: Kernel,
	path: string,
): Promise<void> {
	const deadline = Date.now() + 20_000;
	while (Date.now() < deadline) {
		if (kernel.socketTable.findListener({ path })) {
			return;
		}
		await new Promise((resolveWait) => setTimeout(resolveWait, 20));
	}
	throw new Error(`Timed out waiting for Unix listener on ${path}`);
}

const SOCK_PATH = "/tmp/test.sock";

describeIf(!skipReason(), "WasmVM Unix domain socket integration", { timeout: 30_000 }, () => {
	let ctx: IntegrationKernelResult | undefined;

	afterEach(async () => {
		await ctx?.dispose();
		ctx = undefined;
	});

	it("unix_socket: accept connection, recv data, send pong", async () => {
		ctx = await createIntegrationKernel({
			runtimes: ["wasmvm", "node"],
			commandDirs: [C_BUILD_DIR, COMMANDS_DIR],
		});
		await ctx.kernel.mkdir("/tmp");
		const server = spawnGuestProgram(ctx.kernel, "unix_socket", [SOCK_PATH]);
		await waitForUnixListener(ctx.kernel, SOCK_PATH);

		const client = await runGuestNodeProgram(
			ctx.kernel,
			[
				"const net = require('net');",
				`const client = net.connect({ path: ${JSON.stringify(SOCK_PATH)} }, () => client.write('ping'));`,
				"client.on('data', (chunk) => { console.log(chunk.toString()); client.end(); });",
				"client.on('error', (error) => { console.error(error); process.exit(1); });",
			].join("\n"),
		);
		const serverExit = await server.process.wait();

		expect(client.exitCode).toBe(0);
		expect(client.stderr).toBe("");
		expect(client.stdout.trim()).toBe("pong");
		expect(serverExit).toBe(0);
		expect(decodeChunks(server.stdoutChunks)).toContain(
			`listening on ${SOCK_PATH}`,
		);
		expect(decodeChunks(server.stdoutChunks)).toContain("received: ping");
		expect(decodeChunks(server.stdoutChunks)).toContain("sent: 4");
	});
});
