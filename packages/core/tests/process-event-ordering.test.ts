import { describe, expect, it, vi } from "vitest";
import { NativeSidecarKernelProxy } from "../src/sidecar/rpc-client.js";

const session = { connectionId: "conn-1", sessionId: "sess-1" };
const vm = { vmId: "vm-process-ordering" };

interface PumpEvent {
	ownership: { scope: string; vm_id: string };
	payload: Record<string, unknown>;
}

function createStubClient() {
	const queue: PumpEvent[] = [];
	let notify: (() => void) | null = null;

	const client = {
		async execute() {
			return { processId: "process-1", pid: 4242 };
		},
		async getProcessSnapshot() {
			return [];
		},
		async killProcess() {},
		async writeStdin() {},
		async closeStdin() {},
		async disposeVm() {},
		async dispose() {},
		waitForEvent(
			_filter: unknown,
			_unused: unknown,
			options: { signal: AbortSignal },
		) {
			return new Promise<PumpEvent>((resolve, reject) => {
				const deliver = () => {
					const event = queue.shift();
					if (!event) return false;
					resolve(event);
					return true;
				};
				if (!deliver()) {
					notify = () => {
						if (deliver()) notify = null;
					};
				}
				options.signal.addEventListener("abort", () => {
					reject(new Error("aborted"));
				});
			});
		},
	};

	return {
		client,
		pushEvent(event: PumpEvent) {
			queue.push(event);
			notify?.();
		},
	};
}

function createProxy(client: unknown) {
	return new NativeSidecarKernelProxy({
		client,
		session,
		vm,
		env: {},
		cwd: "/work",
		localMounts: [],
		sidecarMounts: [],
		commandGuestPaths: new Map<string, string>(),
		ownsClient: true,
	} as ConstructorParameters<typeof NativeSidecarKernelProxy>[0]);
}

describe("sidecar-authoritative process event ordering", () => {
	it("awaits execArgv stdin writes before sending EOF", async () => {
		const { client, pushEvent } = createStubClient();
		const operations: string[] = [];
		let finishWrite!: () => void;
		const writeBlocked = new Promise<void>((resolve) => {
			finishWrite = resolve;
		});
		let finishClose!: () => void;
		const closeBlocked = new Promise<void>((resolve) => {
			finishClose = resolve;
		});
		client.writeStdin = vi.fn(async () => {
			operations.push("write:start");
			await writeBlocked;
			operations.push("write:end");
		});
		client.closeStdin = vi.fn(async () => {
			operations.push("close:start");
			await closeBlocked;
			operations.push("close:end");
		});
		const proxy = createProxy(client);
		try {
			const result = proxy.execArgv("node", ["script.js"], {
				stdin: "input",
			});
			await vi.waitFor(() => expect(client.writeStdin).toHaveBeenCalledOnce());
			expect(client.closeStdin).not.toHaveBeenCalled();

			finishWrite();
			await vi.waitFor(() => expect(client.closeStdin).toHaveBeenCalledOnce());
			expect(operations).toEqual(["write:start", "write:end", "close:start"]);

			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_exited",
					process_id: "process-1",
					exit_code: 0,
					stdout: new Uint8Array(),
					stderr: new Uint8Array(),
				},
			});
			let settled = false;
			void result.finally(() => {
				settled = true;
			});
			await Promise.resolve();
			await Promise.resolve();
			expect(settled).toBe(false);

			finishClose();
			await expect(result).resolves.toMatchObject({ exitCode: 0 });
			expect(operations).toEqual([
				"write:start",
				"write:end",
				"close:start",
				"close:end",
			]);
		} finally {
			finishWrite();
			finishClose();
			await proxy.dispose();
		}
	});

	it("propagates execArgv stdin write rejection without sending EOF", async () => {
		const { client, pushEvent } = createStubClient();
		const writeError = new Error("stdin write rejected");
		client.writeStdin = vi.fn(async () => {
			throw writeError;
		});
		client.closeStdin = vi.fn(async () => {});
		const proxy = createProxy(client);
		try {
			const result = proxy.execArgv("node", ["script.js"], { stdin: "input" });
			const rejection = expect(result).rejects.toBe(writeError);
			await vi.waitFor(() => expect(client.writeStdin).toHaveBeenCalledOnce());
			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_exited",
					process_id: "process-1",
					exit_code: 0,
					stdout: new Uint8Array(),
					stderr: new Uint8Array(),
				},
			});
			await rejection;
			expect(client.closeStdin).not.toHaveBeenCalled();
		} finally {
			await proxy.dispose();
		}
	});

	it("propagates execArgv EOF rejection before observing completion", async () => {
		const { client, pushEvent } = createStubClient();
		const closeError = new Error("stdin close rejected");
		client.writeStdin = vi.fn(async () => {});
		client.closeStdin = vi.fn(async () => {
			throw closeError;
		});
		const proxy = createProxy(client);
		try {
			const result = proxy.execArgv("node", ["script.js"], { stdin: "input" });
			const rejection = expect(result).rejects.toBe(closeError);
			await vi.waitFor(() => expect(client.closeStdin).toHaveBeenCalledOnce());
			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_exited",
					process_id: "process-1",
					exit_code: 0,
					stdout: new Uint8Array(),
					stderr: new Uint8Array(),
				},
			});
			await rejection;
		} finally {
			await proxy.dispose();
		}
	});

	it("uses terminal sidecar capture instead of rebuilding output from stream callbacks", async () => {
		const { client, pushEvent } = createStubClient();
		const proxy = createProxy(client);
		try {
			const streamed: string[] = [];
			const result = proxy.execArgv("node", ["script.js"], {
				onStdout(chunk) {
					streamed.push(new TextDecoder().decode(chunk));
				},
			});
			for (let turn = 0; turn < 3; turn += 1) await Promise.resolve();

			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_output",
					process_id: "process-1",
					channel: "stdout",
					chunk: new TextEncoder().encode("callback-only"),
				},
			});
			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_exited",
					process_id: "process-1",
					exit_code: 0,
					stdout: new TextEncoder().encode("sidecar-result"),
					stderr: new TextEncoder().encode("sidecar-stderr"),
				},
			});

			await expect(result).resolves.toEqual({
				exitCode: 0,
				stdout: "sidecar-result",
				stderr: "sidecar-stderr",
			});
			expect(streamed.join("")).toBe("callback-only");
		} finally {
			await proxy.dispose();
		}
	});

	it("completes immediately when ordered output is followed by exit", async () => {
		vi.useFakeTimers();
		const { client, pushEvent } = createStubClient();
		const proxy = createProxy(client);
		try {
			let resolveOutput!: () => void;
			const outputSeen = new Promise<void>((resolve) => {
				resolveOutput = resolve;
			});
			const stdout: string[] = [];
			const proc = await proxy.spawn("node", ["script.js"], {
				onStdout(chunk) {
					stdout.push(new TextDecoder().decode(chunk));
					resolveOutput();
				},
			});

			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_output",
					process_id: "process-1",
					channel: "stdout",
					chunk: new TextEncoder().encode("tail"),
				},
			});
			await outputSeen;

			let settled = false;
			const waiting = proc.wait().then((exitCode) => {
				settled = true;
				return exitCode;
			});
			pushEvent({
				ownership: { scope: "vm", vm_id: vm.vmId },
				payload: {
					type: "process_exited",
					process_id: "process-1",
					exit_code: 0,
				},
			});
			for (let turn = 0; turn < 5; turn += 1) await Promise.resolve();

			expect(stdout.join("")).toBe("tail");
			expect(settled).toBe(true);
			expect(vi.getTimerCount()).toBe(0);
			await expect(waiting).resolves.toBe(0);
		} finally {
			await proxy.dispose();
			vi.useRealTimers();
		}
	});
});
