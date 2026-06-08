import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test, vi } from "vitest";
import type {
	AuthenticatedSession,
	CreatedVm,
	NativeSidecarProcessClient,
} from "../src/sidecar/rpc-client.js";
import { NativeSidecarKernelProxy } from "../src/sidecar/rpc-client.js";

describe("WASM command permission tiers", () => {
	let proxy: NativeSidecarKernelProxy | null = null;
	let fixtureRoot: string | null = null;

	afterEach(async () => {
		await proxy?.dispose();
		proxy = null;
		if (fixtureRoot) {
			rmSync(fixtureRoot, { recursive: true, force: true });
			fixtureRoot = null;
		}
	});

	function createMockClient() {
		let stopped = false;
		const execute = vi.fn(async () => {
			throw new Error("stop after capture");
		});
		const client = {
			waitForEvent: vi.fn(async () => {
				while (!stopped) {
					await new Promise((resolve) => setTimeout(resolve, 1));
				}
				throw new Error("mock stopped");
			}),
			execute,
			disposeVm: vi.fn(async () => {
				stopped = true;
			}),
			dispose: vi.fn(async () => {
				stopped = true;
			}),
		} as unknown as NativeSidecarProcessClient;

		return { client, execute };
	}

	test("sends unresolved WASM commands to the sidecar", async () => {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agent-os-wasm-tiers-"));
		const { client, execute } = createMockClient();

		proxy = new NativeSidecarKernelProxy({
			client,
			session: {
				connectionId: "conn-1",
				sessionId: "session-1",
			} as AuthenticatedSession,
			vm: { vmId: "vm-1" } as CreatedVm,
			env: { HOME: "/workspace" },
			cwd: "/workspace",
			localMounts: [],
			commandGuestPaths: new Map([["grep", "/__agentos/commands/000/grep"]]),
		});

		const proc = proxy.spawn("grep", ["needle", "haystack.txt"], {
			cwd: "/workspace",
		});
		const exitCode = await proc.wait();

		expect(exitCode).toBe(1);
		expect(execute).toHaveBeenCalledTimes(1);
		expect(execute.mock.calls[0]?.[2]).toMatchObject({
			command: "grep",
			args: ["needle", "haystack.txt"],
			cwd: "/workspace",
		});
	});

	test("reports async append redirect flush failures through wait", async () => {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agent-os-wasm-tiers-"));
		let stopped = false;
		const queuedEvents: unknown[] = [];
		const waiters: Array<{
			resolve: (event: unknown) => void;
			reject: (error: Error) => void;
		}> = [];
		const emitEvent = (event: unknown) => {
			const waiter = waiters.shift();
			if (waiter) {
				waiter.resolve(event);
				return;
			}
			queuedEvents.push(event);
		};
		const stopClient = () => {
			stopped = true;
			for (const waiter of waiters.splice(0)) {
				waiter.reject(new Error("mock stopped"));
			}
		};
		const nextEvent = async () => {
			const queued = queuedEvents.shift();
			if (queued) {
				return queued;
			}
			return new Promise<unknown>((resolve, reject) => {
				waiters.push({ resolve, reject });
				if (stopped) {
					reject(new Error("mock stopped"));
				}
			});
		};
		const execute = vi.fn(async (_session, _vm, request) => {
			queueMicrotask(() => {
				emitEvent({
					payload: {
						type: "process_output",
						process_id: request.processId,
						channel: "stdout",
						chunk: new TextEncoder().encode("changed\n"),
					},
				});
				emitEvent({
					payload: {
						type: "process_exited",
						process_id: request.processId,
						exit_code: 0,
					},
				});
			});
			return { pid: 1234 };
		});
		const readFile = vi.fn(async () => {
			const error = new Error("EACCES: permission denied");
			(error as Error & { code?: string }).code = "EACCES";
			throw error;
		});
		const writeFile = vi.fn(async () => {});
		const client = {
			execute,
			readFile,
			writeFile,
			getSignalState: vi.fn(async () => ({ handlers: [] })),
			getProcessSnapshot: vi.fn(async () => [
				{
					processId: "proc-1",
					command: "echo",
					args: ["changed"],
					cwd: "/workspace",
					status: "exited",
					exitCode: 0,
					startTime: Date.now(),
					exitTime: Date.now(),
				},
			]),
			waitForEvent: vi.fn(async () => nextEvent()),
			disposeVm: vi.fn(async () => {
				stopClient();
			}),
			dispose: vi.fn(async () => {
				stopClient();
			}),
		} as unknown as NativeSidecarProcessClient;

		proxy = new NativeSidecarKernelProxy({
			client,
			session: {
				connectionId: "conn-1",
				sessionId: "session-1",
			} as AuthenticatedSession,
			vm: { vmId: "vm-1" } as CreatedVm,
			env: { HOME: "/workspace" },
			cwd: "/workspace",
			localMounts: [],
			commandGuestPaths: new Map([["echo", "/__agentos/commands/000/echo"]]),
		});

		const stderrChunks: Uint8Array[] = [];
		const proc = proxy.spawn("echo changed >> /tmp/write-only.txt", [], {
			shell: true,
			onStderr: (chunk) => stderrChunks.push(chunk),
		});
		const exitCode = await proc.wait();
		const stderr = new TextDecoder().decode(
			Buffer.concat(stderrChunks.map((chunk) => Buffer.from(chunk))),
		);

		expect(exitCode).toBe(1);
		expect(readFile).toHaveBeenCalledWith(
			expect.anything(),
			expect.anything(),
			"/tmp/write-only.txt",
		);
		expect(writeFile).not.toHaveBeenCalled();
		expect(stderr).toMatch(/EACCES|permission/i);
	});
});
